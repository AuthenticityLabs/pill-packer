package codec

import (
	"bytes"
	"encoding/json"
	"io"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func fixture(t *testing.T) (*Layout, [][]byte) {
	t.Helper()
	l, err := Compile(Definition{Identity: "portable/example", Fields: []Field{{Name: "sequence", Type: "U16"}, {Name: "message", Type: "STRING"}}})
	if err != nil {
		t.Fatal(err)
	}
	var frames [][]byte
	for i := 0; i < 3; i++ {
		b, e := l.Encode(nil, []Value{Uint(uint64(i), 2), Text(strings.Repeat("portable codec ", 64))})
		if e != nil {
			t.Fatal(e)
		}
		frames = append(frames, b)
	}
	return l, frames
}
func TestIndependentCodecPACCompression(t *testing.T) {
	l, frames := fixture(t)
	p, e := PackPAC(l, frames, nil, true)
	if e != nil {
		t.Fatal(e)
	}
	packed, e := CompressPAC(p.Bytes, 6)
	if e != nil {
		t.Fatal(e)
	}
	raw, e := ExpandPAC(packed)
	if e != nil {
		t.Fatal(e)
	}
	q, e := ParsePAC(l, raw, nil)
	if e != nil {
		t.Fatal(e)
	}
	for i, m := range q.Members {
		resolved, e := ResolvePAC(l, q.Common, m)
		if e != nil || !bytes.Equal(resolved, frames[i]) {
			t.Fatal("PAC round trip", e)
		}
	}
	for _, frame := range frames {
		var scan Frame
		if e := l.Scan(frame, &scan); e != nil {
			t.Fatal(e)
		}
		var out bytes.Buffer
		if e := l.Write(&out, scan.Values); e != nil || !bytes.Equal(out.Bytes(), frame) {
			t.Fatal("streamed frame differs", e)
		}
	}
	var scan Frame
	if e = l.Scan(frames[0], &scan); e != nil {
		t.Fatal(e)
	}
	if n := testing.AllocsPerRun(100, func() {
		if err := l.Scan(frames[0], &scan); err != nil {
			panic(err)
		}
	}); n != 0 {
		t.Fatalf("reused scan allocated %v", n)
	}
	f := struct {
		Definition, PAC, Compressed []byte
		Frames                      [][]byte
	}{l.Bytes, p.Bytes, packed, frames}
	want, e := json.MarshalIndent(f, "", "  ")
	if e != nil {
		t.Fatal(e)
	}
	want = append(want, '\n')
	path := filepath.Join("..", "..", "npm", "fixtures", "portable.json")
	if os.Getenv("PIL_UPDATE_FIXTURES") == "1" {
		if e = os.MkdirAll(filepath.Dir(path), 0755); e != nil {
			t.Fatal(e)
		}
		if e = os.WriteFile(path, want, 0644); e != nil {
			t.Fatal(e)
		}
	}
	got, e := os.ReadFile(path)
	if e != nil || !bytes.Equal(got, want) {
		t.Fatal("portable interop fixture mismatch; regenerate with PIL_UPDATE_FIXTURES=1", e)
	}
}

type shortWriter struct{}

func (shortWriter) Write(b []byte) (int, error) { return len(b) - 1, nil }
func TestStreamingAndGzipBounds(t *testing.T) {
	l, _ := fixture(t)
	if e := l.Write(shortWriter{}, []Value{Uint(3, 2), Text("x")}); e != io.ErrShortWrite {
		t.Fatal(e)
	}
	raw := bytes.Repeat([]byte("abc"), 100)
	var out bytes.Buffer
	if e := Gzip(&out, bytes.NewReader(raw), 6); e != nil {
		t.Fatal(e)
	}
	got, e := InflateGzip(out.Bytes(), len(raw), len(raw))
	if e != nil || !bytes.Equal(raw, got) {
		t.Fatal(e)
	}
	for _, expected := range []int{-1, len(raw) - 1, len(raw) + 1} {
		if _, e = InflateGzip(out.Bytes(), expected, len(raw)); e == nil {
			t.Fatal("invalid bound accepted")
		}
	}
	if _, e = InflateGzip(append(out.Bytes(), 1), len(raw), len(raw)); e == nil {
		t.Fatal("trailing gzip bytes accepted")
	}
}
func TestPortableColumns(t *testing.T) {
	for w := 1; w <= 8; w++ {
		c := UnsignedEncoding{w, 17}
		value := uint64(17) + 1<<uint(min(w*8-1, 62))
		raw := make([]byte, w)
		PutUnsigned(raw, value, c)
		got, e := GetUnsigned(raw, c)
		if e != nil || got != value {
			t.Fatal(w, got, e)
		}
	}
	if _, e := GetUnsigned([]byte{1}, UnsignedEncoding{1, math.MaxUint64}); e == nil {
		t.Fatal("base overflow accepted")
	}
	raw := []byte{1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12}
	original := bytes.Clone(raw)
	spans := []ColumnSpan{{0, 3, 2}, {6, 2, 3}}
	if e := ShuffleColumns(raw, spans, false); e != nil {
		t.Fatal(e)
	}
	if e := ShuffleColumns(raw, spans, true); e != nil || !bytes.Equal(raw, original) {
		t.Fatal("shuffle changed values", e)
	}
	if e := ShuffleColumns(raw, []ColumnSpan{{0, 3, 2}, {5, 2, 3}}, false); e == nil || !bytes.Equal(raw, original) {
		t.Fatal("invalid spans mutated data")
	}
}
