use crate::{Mode, load};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode as K, KeyEventKind, KeyModifiers as M,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use pil_codec::{
    authoring::{Draft, FieldView, nullable, semantic_type, text_path, type_cddl, with_nullable},
    schema::{SchemaDocument, SchemaType as Ty},
};
use ratatui::{prelude::*, widgets::*};
use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};
use tui_textarea::TextArea;
fn text(s: &str) -> TextArea<'static> {
    TextArea::from(s.lines().map(str::to_owned).collect::<Vec<_>>())
}
fn content(t: &TextArea<'_>) -> String {
    t.lines().join("\n")
}
fn block(title: impl Into<String>, focus: bool) -> Block<'static> {
    Block::bordered()
        .title(title.into())
        .border_style(Style::default().fg(if focus { Color::White } else { Color::DarkGray }))
}
#[derive(Clone, Copy)]
enum Prompt {
    Add,
    Save,
    Open(Mode),
    Root,
    Capacity,
}
enum Dialog {
    Replace(Box<Draft>, Option<PathBuf>),
    Prompt(Prompt, TextArea<'static>),
    Mode(usize),
    Paste(Mode, TextArea<'static>),
    Field(Box<FieldEditor>),
    Dictionary(FieldView, TextArea<'static>),
    Overwrite(PathBuf),
    Quit,
    Remove(FieldView),
}
struct FieldEditor {
    field: FieldView,
    source: String,
    kind: usize,
    preset: usize,
    row: usize,
    min: TextArea<'static>,
    max: TextArea<'static>,
    required: bool,
    null: bool,
    advanced: TextArea<'static>,
}
const KINDS: [&str; 7] = [
    "String", "Number", "Boolean", "Bytes", "Object", "Array", "CDDL",
];
const NUMBERS: [&str; 8] = [
    "Signed 64-bit",
    "Unsigned 64-bit",
    "0–255 · 1 Byte",
    "0–65,535 · 2 Bytes",
    "0–16,777,215 · 3 Bytes",
    "0–4,294,967,295 · 4 Bytes",
    "Floating Point · 8 Bytes",
    "Custom Range",
];
impl FieldEditor {
    fn new(field: FieldView, source: &str) -> Self {
        let base = with_nullable(field.value.clone(), false);
        let kind = match semantic_type(&base) {
            "String" => 0,
            "Number" => 1,
            "Boolean" => 2,
            "Bytes" => 3,
            "Object" => 4,
            "Array" => 5,
            _ => 6,
        };
        let (mut preset, mut min, mut max) = (0, String::new(), String::new());
        match &base {
            Ty::Uint => preset = 1,
            Ty::Float => preset = 6,
            Ty::Integer { min: a, max: b } => {
                min = a.to_string();
                max = b.to_string();
                preset = match (*a, *b) {
                    (0, 255) => 2,
                    (0, 65535) => 3,
                    (0, 16777215) => 4,
                    (0, 4294967295) => 5,
                    (a, b) if a == i64::MIN as i128 && b == i64::MAX as i128 => 0,
                    _ => 7,
                }
            }
            Ty::Size { min: a, max: b, .. } => {
                preset = 1;
                min = a.to_string();
                max = b.to_string();
            }
            _ => (),
        }
        Self {
            source: source.into(),
            kind,
            preset,
            row: 0,
            min: text(&min),
            max: text(&max),
            required: !field.optional,
            null: nullable(&field.value),
            advanced: text(&type_cddl(&base, 0)),
            field,
        }
    }
    fn ty(&self) -> Result<Ty, String> {
        let bounds = || -> Result<(i128, i128), String> {
            let min = content(&self.min)
                .trim()
                .parse::<i128>()
                .map_err(|_| "Enter an exact integer minimum")?;
            let max = content(&self.max)
                .trim()
                .parse::<i128>()
                .map_err(|_| "Enter an exact integer maximum")?;
            if min > max {
                return Err("Minimum exceeds maximum".into());
            }
            Ok((min, max))
        };
        let base = match self.kind {
            0 | 3 => {
                let value = if self.kind == 0 { Ty::Text } else { Ty::Bytes };
                if self.preset % 2 == 0 {
                    value
                } else {
                    let (min, max) = bounds()?;
                    Ty::Size {
                        value: Box::new(value),
                        min: usize::try_from(min).map_err(|_| "Invalid byte length")?,
                        max: usize::try_from(max).map_err(|_| "Invalid byte length")?,
                    }
                }
            }
            1 => match self.preset % 8 {
                0 => Ty::Integer {
                    min: i64::MIN as i128,
                    max: i64::MAX as i128,
                },
                1 => Ty::Uint,
                2..=5 => Ty::Integer {
                    min: 0,
                    max: [255, 65535, 16777215, 4294967295][self.preset - 2],
                },
                6 => Ty::Float,
                _ => {
                    let (min, max) = bounds()?;
                    Ty::Integer { min, max }
                }
            },
            2 => Ty::Bool,
            _ => {
                let mut name = "doc-edit-value".to_string();
                while self.source.contains(&name) {
                    name.push_str("-x");
                }
                let s = SchemaDocument::from_cddl(
                    &format!("{}\n{name} = {}", self.source, content(&self.advanced)),
                    &name,
                )
                .map_err(|e| e.to_string())?;
                s.types[&name].clone()
            }
        };
        Ok(with_nullable(base, self.null))
    }
    fn step(&mut self, forward: bool) {
        match self.row {
            0 => {
                self.kind = (self.kind + if forward { 1 } else { 6 }) % 7;
                self.preset = 0;
                match self.kind {
                    4 => self.advanced = text("{ * tstr => any }"),
                    5 => self.advanced = text("[ * tstr ]"),
                    _ => (),
                }
            }
            1 => {
                let n = if self.kind == 1 { 8 } else { 2 };
                self.preset = (self.preset + if forward { 1 } else { n - 1 }) % n;
            }
            4 => self.required = !self.required,
            5 => self.null = !self.null,
            _ => (),
        }
    }
}
struct App {
    draft: Draft,
    editor: TextArea<'static>,
    fields: Vec<FieldView>,
    selected: usize,
    tab: usize,
    dialog: Option<Dialog>,
    status: String,
    dirty: bool,
    output: Option<PathBuf>,
    quit: bool,
    last_edit: Option<Instant>,
    list: ListState,
    field_rect: Rect,
}
impl App {
    fn new(draft: Draft, mode: Option<Mode>, output: Option<PathBuf>) -> Self {
        let editor = text(&draft.source);
        let fields = draft.fields().unwrap_or_default();
        let native = draft.native;
        let dialog = mode.map(|m| {
            if matches!(m, Mode::Rx) {
                Dialog::Prompt(Prompt::Open(m), text(""))
            } else {
                Dialog::Paste(m, text(""))
            }
        });
        Self {
            draft,
            editor,
            fields,
            selected: 0,
            tab: if native { 1 } else { 0 },
            dialog,
            status: "Select a field to review its type and constraints.".into(),
            dirty: false,
            output,
            quit: false,
            last_edit: None,
            list: ListState::default(),
            field_rect: Rect::default(),
        }
    }
    fn imported(&mut self, draft: Draft, path: Option<PathBuf>) {
        if self.dirty {
            self.dialog = Some(Dialog::Replace(Box::new(draft), path));
        } else {
            self.draft = draft;
            self.output = path;
            self.refresh();
            self.status = "Opened. Review fields before saving.".into();
        }
    }
    fn selected(&self) -> Option<FieldView> {
        self.fields.get(self.selected).cloned()
    }
    fn refresh(&mut self) {
        if self.draft.native {
            self.tab = 1;
        }
        self.editor = text(&self.draft.source);
        self.fields = self.draft.fields().unwrap_or_default();
        self.selected = self.selected.min(self.fields.len().saturating_sub(1));
        self.dirty = true;
    }
    fn sync(&mut self) {
        self.draft.source = content(&self.editor);
        match self.draft.fields() {
            Ok(fields) => {
                self.fields = fields;
                self.selected = self.selected.min(self.fields.len().saturating_sub(1));
                self.status = if self.draft.native {
                    "Native Definition JSON. Existing layout and annotations are retained."
                } else {
                    "CDDL parsed. F5 validates samples; F6 accepts reviewed CDDL decisions."
                }
                .into();
            }
            Err(e) => self.status = e.to_string(),
        };
        self.last_edit = None;
    }
    fn save(&mut self, path: PathBuf, overwrite: bool) {
        match self.draft.save(&path, overwrite) {
            Ok(p) => {
                self.output = Some(p.clone());
                self.status = format!("Saved {}", p.display());
                self.dirty = false;
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    fn event(&mut self, event: Event) {
        if matches!(&event,Event::Key(k) if k.kind!=KeyEventKind::Release && k.modifiers.contains(M::CONTROL) && matches!(k.code,K::Char('q')|K::Char('c')))
        {
            if self.dirty || self.dialog.is_some() {
                self.dialog = Some(Dialog::Quit);
            } else {
                self.quit = true;
            }
            return;
        }
        if let Some(dialog) = self.dialog.take() {
            self.dialog_event(dialog, event);
            return;
        }
        match event {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                if k.modifiers.contains(M::CONTROL) {
                    match k.code {
                        K::Char('q') | K::Char('c') => {
                            if self.dirty {
                                self.dialog = Some(Dialog::Quit)
                            } else {
                                self.quit = true
                            };
                            return;
                        }
                        K::Char('s') => {
                            self.sync();
                            self.dialog = Some(Dialog::Prompt(
                                Prompt::Save,
                                text(
                                    &self
                                        .output
                                        .as_ref()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_else(|| format!("{}.rx", self.draft.root)),
                                ),
                            ));
                            return;
                        }
                        _ => (),
                    }
                }
                match k.code {
                    K::F(2) => {
                        self.sync();
                        self.tab = (self.tab + 1) % 4;
                    }
                    K::F(3) => self.dialog = Some(Dialog::Mode(0)),
                    K::F(5) => {
                        self.sync();
                        self.status = match self.draft.validate_samples() {
                            Ok(bytes) => format!(
                                "{} samples validated · {} encoded bytes · {} decisions to review",
                                self.draft.samples.len(),
                                bytes,
                                self.draft.pending.len()
                            ),
                            Err(e) => e.to_string(),
                        };
                    }
                    K::F(6) => {
                        if self.tab == 1 {
                            self.sync();
                            match self.draft.validate_samples() {
                                Ok(_) => {
                                    self.draft.pending.clear();
                                    self.status =
                                        "Current CDDL explicitly accepted. Ctrl+S builds RX."
                                            .into();
                                    self.dirty = true;
                                }
                                Err(e) => self.status = e.to_string(),
                            }
                        } else if let Some(f) = self.selected() {
                            self.dialog = Some(Dialog::Field(Box::new(FieldEditor::new(
                                f,
                                &self.draft.source,
                            ))));
                        }
                    }
                    _ if self.tab == 1 => {
                        self.editor.input(k);
                        self.last_edit = Some(Instant::now());
                        self.dirty = true;
                    }
                    K::Down | K::Char('j') => {
                        self.selected = (self.selected + 1).min(
                            if self.tab == 2 {
                                self.draft.samples.len()
                            } else {
                                self.fields.len()
                            }
                            .saturating_sub(1),
                        )
                    }
                    K::Up | K::Char('k') => self.selected = self.selected.saturating_sub(1),
                    K::Enter if self.tab == 0 => {
                        if let Some(f) = self.selected() {
                            self.dialog = Some(Dialog::Field(Box::new(FieldEditor::new(
                                f,
                                &self.draft.source,
                            ))));
                        }
                    }
                    K::Char('a') if self.tab == 0 => {
                        self.dialog = Some(Dialog::Prompt(Prompt::Add, text("")))
                    }
                    K::Delete if self.tab == 0 => {
                        if let Some(f) = self.selected() {
                            self.dialog = Some(Dialog::Remove(f));
                        }
                    }
                    K::Char('o') if self.tab == 0 => {
                        let result = self.draft.schema().and_then(|s| {
                            self.draft
                                .set_open(!matches!(s.types[&s.root], Ty::OpenRecord { .. }))
                        });
                        match result {
                            Ok(()) => {
                                self.refresh();
                                self.status =
                                    "Additional-fields policy changed. F5 validates samples.".into()
                            }
                            Err(e) => self.status = e.to_string(),
                        }
                    }
                    K::Char('d') if self.tab == 0 => {
                        if let Some(f) = self.selected() {
                            if let Some(path) = text_path(&f.path, &f.value) {
                                let values = self
                                    .draft
                                    .dictionaries
                                    .get(&path)
                                    .cloned()
                                    .unwrap_or_else(|| {
                                        let mut v: Vec<_> = self
                                            .draft
                                            .observed
                                            .get(&f.evidence_path)
                                            .map(|s| s.strings.iter().collect())
                                            .unwrap_or_default();
                                        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                                        v.into_iter().take(256).map(|(s, _)| s.clone()).collect()
                                    });
                                let json = serde_json::to_string_pretty(&values).unwrap();
                                self.dialog = Some(Dialog::Dictionary(f, text(&json)));
                            } else {
                                self.status = "Select a text field for a dictionary.".into();
                            }
                        }
                    }
                    K::Char('f') if self.tab == 0 => {
                        self.dialog = Some(Dialog::Prompt(Prompt::Capacity, text("")))
                    }
                    K::Char('r') if self.tab == 0 => {
                        self.dialog = Some(Dialog::Prompt(Prompt::Root, text(&self.draft.root)))
                    }
                    _ => (),
                }
            }
            Event::Paste(s) if self.tab == 1 => {
                self.editor.insert_str(s);
                self.last_edit = Some(Instant::now());
                self.dirty = true;
            }
            Event::Mouse(m) => match m.kind {
                event::MouseEventKind::ScrollDown => {
                    self.selected = (self.selected + 3).min(self.fields.len().saturating_sub(1))
                }
                event::MouseEventKind::ScrollUp => self.selected = self.selected.saturating_sub(3),
                event::MouseEventKind::Down(event::MouseButton::Left)
                    if self.tab == 0
                        && self.field_rect.contains(Position::new(m.column, m.row)) =>
                {
                    self.selected = (m.row.saturating_sub(self.field_rect.y + 1) as usize
                        + self.list.offset())
                    .min(self.fields.len().saturating_sub(1));
                }
                _ => (),
            },
            _ => (),
        }
    }
    fn dialog_event(&mut self, mut dialog: Dialog, event: Event) {
        if matches!(&event,Event::Key(k) if k.kind==KeyEventKind::Release) {
            self.dialog = Some(dialog);
            return;
        }
        if matches!(&event,Event::Key(k) if k.code==K::Esc) {
            return;
        }
        let key = if let Event::Key(k) = event {
            Some(k)
        } else {
            None
        };
        match &mut dialog {
            Dialog::Replace(d, path) => {
                if key.is_some_and(|k| k.code == K::Char('y')) {
                    self.dirty = false;
                    self.imported(*d.clone(), path.clone());
                    return;
                }
            }
            Dialog::Quit => {
                if key.is_some_and(|k| k.code == K::Char('y')) {
                    self.quit = true;
                    return;
                }
            }
            Dialog::Overwrite(path) => {
                if key.is_some_and(|k| k.code == K::Char('y')) {
                    self.save(path.clone(), true);
                    return;
                }
            }
            Dialog::Remove(f) => {
                if key.is_some_and(|k| k.code == K::Char('y')) {
                    match self.draft.remove(f) {
                        Ok(()) => self.refresh(),
                        Err(e) => self.status = e.to_string(),
                    };
                    return;
                }
            }
            Dialog::Mode(i) => {
                if let Some(k) = key {
                    match k.code {
                        K::Down | K::Right => *i = (*i + 1) % 3,
                        K::Up | K::Left => *i = (*i + 2) % 3,
                        K::Enter => {
                            self.dialog = Some(Dialog::Prompt(
                                Prompt::Open([Mode::Json, Mode::Cddl, Mode::Rx][*i]),
                                text(""),
                            ));
                            return;
                        }
                        _ => (),
                    }
                }
            }
            Dialog::Prompt(kind, input) => {
                if key.is_some_and(|k| k.code == K::Enter) {
                    let value = content(input);
                    let value = value.trim();
                    match kind {
                        Prompt::Add => match self.draft.add(value) {
                            Ok(()) => self.refresh(),
                            Err(e) => {
                                self.status = e.to_string();
                                self.dialog = Some(dialog);
                                return;
                            }
                        },
                        Prompt::Save => {
                            let p = PathBuf::from(value);
                            if p.extension().and_then(|e| e.to_str()) != Some("rx") {
                                self.status = "Output must use .rx".into();
                                self.dialog = Some(dialog);
                                return;
                            }
                            if p.exists() {
                                self.dialog = Some(Dialog::Overwrite(p));
                                return;
                            }
                            self.save(p, false);
                        }
                        Prompt::Open(mode) => {
                            if value.is_empty() && !matches!(mode, Mode::Rx) {
                                self.dialog = Some(Dialog::Paste(*mode, text("")));
                                return;
                            }
                            match load(*mode, value, None) {
                                Ok(d) => {
                                    self.imported(
                                        d,
                                        Some(PathBuf::from(value).with_extension("rx")),
                                    );
                                    return;
                                }
                                Err(e) => {
                                    self.status = e;
                                    self.dialog = Some(dialog);
                                    return;
                                }
                            }
                        }
                        Prompt::Root => {
                            let old = self.draft.root.clone();
                            self.draft.root = value.into();
                            if let Err(e) = self.draft.schema() {
                                self.draft.root = old;
                                self.status = e.to_string();
                                self.dialog = Some(dialog);
                                return;
                            }
                            self.refresh();
                        }
                        Prompt::Capacity => {
                            if let Some(f) = self.selected() {
                                let capacity = if value.is_empty() {
                                    None
                                } else {
                                    match value.parse::<u32>() {
                                        Ok(n) if n > 0 => Some(n),
                                        _ => {
                                            self.status="Enter a positive byte capacity, or leave blank for variable text.".into();
                                            self.dialog = Some(dialog);
                                            return;
                                        }
                                    }
                                };
                                match self.draft.fixed_capacity(&f, capacity) {
                                    Ok(()) => {
                                        self.dirty = true;
                                        self.status = "Storage capacity updated.".into();
                                    }
                                    Err(e) => {
                                        self.status = e.to_string();
                                        self.dialog = Some(dialog);
                                        return;
                                    }
                                }
                            }
                        }
                    };
                    return;
                }
                input.input(event);
            }
            Dialog::Paste(mode, input) => {
                if key.is_some_and(|k| k.code == K::F(5)) {
                    let result = match mode {
                        Mode::Json => Draft::from_json(&content(input)),
                        _ => Draft::from_cddl(content(input), None),
                    };
                    match result {
                        Ok(d) => {
                            self.imported(d, None);
                            return;
                        }
                        Err(e) => self.status = e.to_string(),
                    }
                } else {
                    input.input(event);
                }
            }
            Dialog::Dictionary(field, input) => {
                if key.is_some_and(|k| k.code == K::F(5)) {
                    match serde_json::from_str::<Vec<String>>(&content(input))
                        .map_err(|e| e.to_string())
                        .and_then(|values| {
                            self.draft
                                .dictionary(field, values)
                                .map_err(|e| e.to_string())
                        }) {
                        Ok(()) => {
                            self.dirty = true;
                            self.status =
                                "Dictionary updated. Other strings remain valid; [] disables it."
                                    .into();
                            return;
                        }
                        Err(e) => self.status = e,
                    }
                } else {
                    input.input(event);
                }
            }
            Dialog::Field(form) => {
                if key.is_some_and(|k| {
                    k.modifiers.contains(M::CONTROL) && k.code == K::Enter || k.code == K::F(5)
                }) {
                    let result = form.ty().and_then(|ty| {
                        self.draft
                            .apply(&form.field, ty, !form.required)
                            .map_err(|e| e.to_string())
                    });
                    match result {
                        Ok(()) => {
                            self.refresh();
                            self.status = "Field decision applied. F5 checks all samples.".into();
                            return;
                        }
                        Err(e) => self.status = e,
                    }
                } else if let Some(k) = key {
                    match k.code {
                        K::Tab => form.row = (form.row + 1) % 7,
                        K::BackTab => form.row = (form.row + 6) % 7,
                        K::Up if !matches!(form.row, 2 | 3 | 6) => form.row = (form.row + 6) % 7,
                        K::Down if !matches!(form.row, 2 | 3 | 6) => form.row = (form.row + 1) % 7,
                        K::Left | K::Right | K::Char(' ') if matches!(form.row, 0 | 1 | 4 | 5) => {
                            form.step(k.code != K::Left)
                        }
                        _ => match form.row {
                            2 => {
                                form.min.input(k);
                            }
                            3 => {
                                form.max.input(k);
                            }
                            6 => {
                                form.advanced.input(k);
                            }
                            _ => (),
                        },
                    }
                } else if let Event::Paste(s) = event {
                    match form.row {
                        2 => {
                            form.min.insert_str(s);
                        }
                        3 => {
                            form.max.insert_str(s);
                        }
                        6 => {
                            form.advanced.insert_str(s);
                        }
                        _ => (),
                    }
                }
            }
        }
        self.dialog = Some(dialog);
    }
    fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        if area.width < 60 || area.height < 18 {
            f.render_widget(Paragraph::new("DOC needs a terminal at least 60 columns × 18 rows. Resize to continue. Ctrl+Q exits.").wrap(Wrap{trim:false}),area);
            return;
        }
        let rows = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .split(area);
        let titles = [
            "Fields",
            if self.draft.native {
                "Definition JSON"
            } else {
                "CDDL"
            },
            "Samples",
            "Layout",
        ];
        f.render_widget(
            Tabs::new(titles)
                .select(self.tab)
                .block(Block::default().title(format!(
                    "DOC · {}{}",
                    self.draft.root,
                    if self.dirty { " *" } else { "" }
                )))
                .highlight_style(Style::default().fg(Color::Black).bg(Color::Gray)),
            rows[0],
        );
        let columns = Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(rows[1]);
        match self.tab {
            0 => {
                self.field_rect = columns[0];
                let items: Vec<_> = self
                    .fields
                    .iter()
                    .map(|v| {
                        ListItem::new(format!(
                            "{} {} · {}",
                            if self.draft.pending.contains(&v.evidence_path) {
                                "?"
                            } else {
                                "✓"
                            },
                            v.path
                                .strip_prefix(&format!("{}.", self.draft.root))
                                .unwrap_or(&v.path),
                            semantic_type(&v.value)
                        ))
                    })
                    .collect();
                self.list.select(if items.is_empty() {
                    None
                } else {
                    Some(self.selected)
                });
                f.render_stateful_widget(
                    List::new(items)
                        .block(block("Fields · Enter to Edit", true))
                        .highlight_style(Style::default().fg(Color::Black).bg(Color::Gray)),
                    columns[0],
                    &mut self.list,
                );
                let open = self
                    .draft
                    .schema()
                    .ok()
                    .is_some_and(|s| matches!(s.types[&s.root], Ty::OpenRecord { .. }));
                let mut lines = vec![
                    format!(
                        "Additional Fields: {}",
                        if open { "Preserve" } else { "Reject" }
                    ),
                    format!(
                        "{} Samples · {} Decisions to Review",
                        self.draft.samples.len(),
                        self.draft.pending.len()
                    ),
                    String::new(),
                ];
                if let Some(field) = self.selected() {
                    lines.extend([
                        field.path.clone(),
                        format!("Type: {}", semantic_type(&field.value)),
                        format!(
                            "Presence: {}",
                            if field.optional {
                                "Optional"
                            } else {
                                "Required"
                            }
                        ),
                        format!(
                            "Null: {}",
                            if nullable(&field.value) {
                                "Allowed"
                            } else {
                                "Rejected"
                            }
                        ),
                        format!("CDDL: {}", type_cddl(&field.value, 0)),
                        String::new(),
                    ]);
                    if let Some(o) = self.draft.observed.get(&field.evidence_path) {
                        lines.extend([
                            format!("Observed: {} / {} · Null: {}", o.present, o.total, o.nulls),
                            format!(
                                "Types: {}",
                                o.kinds.iter().cloned().collect::<Vec<_>>().join(", ")
                            ),
                            format!(
                                "Distinct Strings: {}{}",
                                o.strings.len(),
                                if o.truncated { "+" } else { "" }
                            ),
                            format!("Longest Text: {} UTF-8 Bytes", o.max_bytes),
                        ]);
                    }
                    if let Some(path) = text_path(&field.path, &field.value) {
                        lines.push(format!(
                            "Dictionary: {} Entries",
                            self.draft.dictionaries.get(&path).map_or(0, Vec::len)
                        ));
                        lines.push(format!(
                            "Fixed Text Capacity: {}",
                            self.draft
                                .fixed_strings
                                .get(&path)
                                .map(|n| format!("{n} Bytes"))
                                .unwrap_or("Off".into())
                        ));
                    }
                }
                f.render_widget(
                    Paragraph::new(lines.join("\n"))
                        .wrap(Wrap { trim: false })
                        .block(block("Inspector", false)),
                    columns[1],
                );
            }
            1 => {
                self.editor.set_block(block(
                    if self.draft.native {
                        "Native Definition JSON · No CDDL Constraints Inferred"
                    } else {
                        "CDDL · F6 Accept Reviewed Source"
                    },
                    true,
                ));
                self.editor
                    .set_line_number_style(Style::default().fg(Color::DarkGray));
                f.render_widget(&self.editor, rows[1]);
            }
            2 => {
                let sample = self
                    .draft
                    .samples
                    .get(
                        self.selected
                            .min(self.draft.samples.len().saturating_sub(1)),
                    )
                    .map(|v| serde_json::to_string_pretty(v).unwrap())
                    .unwrap_or("No samples imported.".into());
                f.render_widget(
                    Paragraph::new(sample)
                        .wrap(Wrap { trim: false })
                        .block(block("Samples · Arrow Keys", true)),
                    rows[1],
                );
            }
            _ => {
                let native_layout = self.draft.native_document().ok().map(|d| {
                    d.definition()
                        .fields()
                        .iter()
                        .enumerate()
                        .map(|(i, f)| format!("{i:>3}  {:<24} {:?}", f.name, f.kind))
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                let value = if self.draft.native {
                    native_layout.unwrap_or("Invalid Definition JSON".into())
                } else {
                    match self.draft.schema() {
                        Ok(s) => match s.compile() {
                            Ok(c) => c
                                .layout()
                                .fields()
                                .iter()
                                .enumerate()
                                .map(|(i, f)| {
                                    format!(
                                        "{i:>3}  {:<24} {:?}  {} dictionary values",
                                        f.name,
                                        f.kind,
                                        f.values.len()
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                            Err(e) => e.to_string(),
                        },
                        Err(e) => e.to_string(),
                    }
                };
                f.render_widget(
                    Paragraph::new(value)
                        .wrap(Wrap { trim: false })
                        .block(block("Compiled PIL Layout", false)),
                    rows[1],
                );
            }
        }
        f.render_widget(
            Paragraph::new(&*self.status)
                .wrap(Wrap { trim: false })
                .block(block("Status", false)),
            rows[2],
        );
        let help = if self.tab == 0 {
            "Enter Edit · A Add · Del Remove · D Dictionary · F Capacity · O Additional Fields\nF2 View · F3 Open · F5 Validate · Ctrl+S Save RX · Ctrl+Q Quit"
        } else {
            "F2 View · F3 Open · F5 Validate · F6 Accept Reviewed CDDL\nCtrl+S Save RX · Ctrl+Q Quit"
        };
        f.render_widget(
            Paragraph::new(help).style(Style::default().fg(Color::Gray)),
            rows[3],
        );
        if let Some(dialog) = &mut self.dialog {
            draw_dialog(
                dialog,
                f,
                Rect::new(area.x, area.y, area.width, area.height.saturating_sub(5)),
            );
        }
    }
}
fn popup(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(4));
    let h = height.min(area.height.saturating_sub(4));
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}
fn draw_dialog(dialog: &mut Dialog, f: &mut Frame, area: Rect) {
    let rect = popup(
        area,
        90,
        match dialog {
            Dialog::Field(_) => 28,
            Dialog::Paste(..) | Dialog::Dictionary(..) => 30,
            _ => 9,
        },
    );
    f.render_widget(Clear, rect);
    match dialog {
        Dialog::Mode(i) => {
            f.render_widget(
                List::new(
                    ["JSON Samples", "CDDL Source", "RX Definition"]
                        .iter()
                        .enumerate()
                        .map(|(n, s)| {
                            ListItem::new(format!("{} {s}", if n == *i { "›" } else { " " }))
                        }),
                )
                .block(block("Open · Arrows / Enter", true)),
                rect,
            );
        }
        Dialog::Prompt(kind, input) => {
            let title = match kind {
                Prompt::Add => "Add Field · Name",
                Prompt::Save => "Save RX · Output Must End in .rx",
                Prompt::Open(Mode::Json) => "Open JSON · File Path (Blank to Paste)",
                Prompt::Open(Mode::Cddl) => "Open CDDL · File Path (Blank to Paste)",
                Prompt::Open(Mode::Rx) => "Open RX · File Path",
                Prompt::Root => "CDDL Root Rule",
                Prompt::Capacity => "UTF-8 Byte Capacity · Blank Disables · Requires Bounded Text",
            };
            input.set_block(block(format!("{title} · Enter / Esc"), true));
            f.render_widget(&*input, rect);
        }
        Dialog::Paste(mode, input) => {
            input.set_block(block(
                format!(
                    "Paste {} · F5 Import / Esc",
                    if matches!(mode, Mode::Json) {
                        "JSON Samples"
                    } else {
                        "CDDL"
                    }
                ),
                true,
            ));
            f.render_widget(&*input, rect);
        }
        Dialog::Dictionary(_, input) => {
            input.set_block(block(
                "Dictionary · JSON String Array · F5 Apply · [] Disables",
                true,
            ));
            f.render_widget(&*input, rect);
        }
        Dialog::Replace(..) => f.render_widget(
            Paragraph::new(
                "Discard unsaved changes and open the other input?\n\nY Replace · Esc Keep Editing",
            )
            .block(block("Unsaved Changes", true)),
            rect,
        ),
        Dialog::Quit => f.render_widget(
            Paragraph::new("Discard unsaved changes and quit?\n\nY Discard · Esc Keep Editing")
                .block(block("Unsaved Changes", true)),
            rect,
        ),
        Dialog::Overwrite(p) => f.render_widget(
            Paragraph::new(format!(
                "Replace {}?\n\nY Replace · Esc Cancel",
                p.display()
            ))
            .block(block("Replace RX", true)),
            rect,
        ),
        Dialog::Remove(field) => f.render_widget(
            Paragraph::new(format!("Remove {}?\n\nY Remove · Esc Cancel", field.path))
                .block(block("Remove Field", true)),
            rect,
        ),
        Dialog::Field(edit) => {
            f.render_widget(
                block(
                    format!("{} · Tab / Arrows · F5 Apply · Esc Cancel", edit.field.path),
                    true,
                ),
                rect,
            );
            let inner = rect.inner(Margin::new(2, 1));
            let visible = (inner.height as usize / 3).clamp(1, 7);
            let start = edit.row.saturating_sub(visible - 1).min(7 - visible);
            let rows = Layout::vertical((0..visible).map(|_| Constraint::Length(3))).split(inner);
            for (index, area) in rows.iter().enumerate() {
                let row = start + index;
                let label = match row {
                    0 => format!("Type: {}", KINDS[edit.kind]),
                    1 => format!(
                        "Range: {}",
                        if edit.kind == 1 {
                            NUMBERS[edit.preset % 8]
                        } else if matches!(edit.kind, 0 | 3) {
                            if edit.preset % 2 == 0 {
                                "Unbounded"
                            } else {
                                "Byte Length Range"
                            }
                        } else {
                            "Use CDDL Below"
                        }
                    ),
                    4 => format!("Required: {}", if edit.required { "Yes" } else { "No" }),
                    5 => format!("Allow Null: {}", if edit.null { "Yes" } else { "No" }),
                    _ => String::new(),
                };
                match row {
                    2 => {
                        edit.min.set_block(block(
                            "Minimum · Custom Range / Byte Length",
                            edit.row == row,
                        ));
                        f.render_widget(&edit.min, *area);
                    }
                    3 => {
                        edit.max.set_block(block(
                            "Maximum · Custom Range / Byte Length",
                            edit.row == row,
                        ));
                        f.render_widget(&edit.max, *area);
                    }
                    6 => {
                        edit.advanced.set_block(block(
                            "CDDL · Object / Array / Advanced Type",
                            edit.row == row,
                        ));
                        f.render_widget(&edit.advanced, *area);
                    }
                    _ => f.render_widget(
                        Paragraph::new(format!(
                            "{} {}",
                            if edit.row == row { "›" } else { " " },
                            label
                        ))
                        .style(if edit.row == row {
                            Style::default().fg(Color::Black).bg(Color::Gray)
                        } else {
                            Style::default()
                        }),
                        *area,
                    ),
                }
            }
        }
    }
}
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
    }
}
pub fn run(draft: Draft, mode: Option<Mode>, output: Option<PathBuf>) -> Result<(), String> {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        original(info);
    }));
    enable_raw_mode().map_err(|e| e.to_string())?;
    let _restore = Restore;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .map_err(|e| e.to_string())?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|e| e.to_string())?;
    let mut app = App::new(draft, mode, output);
    while !app.quit {
        terminal.draw(|f| app.draw(f)).map_err(|e| e.to_string())?;
        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            app.event(event::read().map_err(|e| e.to_string())?);
        }
        if app
            .last_edit
            .is_some_and(|t| t.elapsed() > Duration::from_millis(350))
        {
            app.sync();
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    fn key(code: K, mods: M) -> Event {
        Event::Key(KeyEvent::new(code, mods))
    }
    #[test]
    fn draw_resizes_and_field_editor_uses_ranges() {
        for (width, height) in [(120, 38), (60, 18), (42, 12)] {
            let d = Draft::from_json(r#"{"status":"ok","count":42}"#).unwrap();
            let mut a = App::new(d, None, None);
            let mut t = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            t.draw(|f| a.draw(f)).unwrap();
            if width >= 60 {
                a.event(key(K::Enter, M::NONE));
                assert!(matches!(a.dialog, Some(Dialog::Field(_))));
                t.draw(|f| a.draw(f)).unwrap();
            }
        }
    }
    #[test]
    fn form_and_save_produce_rx() {
        let d = Draft::from_json(r#"{"count":42}"#).unwrap();
        let mut a = App::new(d, None, None);
        a.event(key(K::Enter, M::NONE));
        let Some(Dialog::Field(f)) = &mut a.dialog else {
            panic!()
        };
        f.preset = 2;
        a.event(key(K::F(5), M::NONE));
        assert!(a.draft.pending.is_empty());
        let c = a.draft.schema().unwrap().compile().unwrap();
        assert_eq!(
            c.encode_json(&serde_json::json!({"count":42}))
                .unwrap()
                .len(),
            2
        ); // one state byte and one U8; the additional field is absent
        a.event(key(K::Char('s'), M::CONTROL));
        assert!(matches!(a.dialog, Some(Dialog::Prompt(Prompt::Save, _))));
    }
    #[test]
    fn incomplete_source_is_retained_and_errors_do_not_exit() {
        let mut a = App::new(Draft::default(), None, None);
        a.tab = 1;
        a.editor = text("broken = {");
        a.event(key(K::F(5), M::NONE));
        assert_eq!(a.draft.source, "broken = {");
        assert!(!a.quit);
        assert!(a.draft.bytes().is_err());
        a.event(key(K::F(2), M::NONE));
        assert_eq!(a.draft.source, "broken = {");
    }
    #[test]
    fn quitting_an_input_dialog_is_possible() {
        let mut a = App::new(Draft::default(), Some(Mode::Json), None);
        a.event(key(K::Char('q'), M::CONTROL));
        assert!(matches!(a.dialog, Some(Dialog::Quit)));
        a.event(key(K::Char('y'), M::NONE));
        assert!(a.quit);
    }
    #[test]
    fn native_rx_uses_definition_json_editor() {
        let doc = pil_codec::DefinitionDocument::from_json(
            br#"{"pil":0,"definition":"test","version":0,"fields":[{"name":"id","type":"UUID"}]}"#,
        )
        .unwrap();
        let mut a = App::new(Draft::from_rx(doc.as_pil()).unwrap(), None, None);
        assert_eq!(a.tab, 1);
        a.sync();
        assert_eq!(a.draft.bytes().unwrap(), doc.as_pil());
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
    }
}
