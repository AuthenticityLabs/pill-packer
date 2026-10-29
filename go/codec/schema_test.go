package codec

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestNestedSchemaSharedRuntime(t *testing.T) {
	read := func(name string) []byte {
		t.Helper()
		b, e := os.ReadFile(filepath.Join("..", "..", "rust", "tests", "fixtures", "schema", name))
		if e != nil {
			t.Fatal(e)
		}
		return b
	}
	l, err := Parse(read("purchase.rx"), IdentityProfile{Primary: -1})
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := l.EncodeJSON(read("record.json"))
	if err != nil || !bytes.Equal(encoded, read("record.pil")) {
		t.Fatal("shared encoding differs", err)
	}
	jsonView, err := l.DecodeJSON(encoded)
	if err != nil || !bytes.Contains(jsonView, []byte("18446744073709551615")) {
		t.Fatal("U64 precision lost", err)
	}
	var frame Frame
	if err = l.Scan(encoded, &frame); err != nil {
		t.Fatal(err)
	}
	invalid := read("invalid-child.pil")
	if err = l.Scan(invalid, &frame); err == nil {
		t.Fatal("accepted invalid nested quantity")
	}
	if err = l.ScanPlain(invalid, &frame); err == nil {
		t.Fatal("plain scan skipped nested constraints")
	}
	// A fragment can be structurally sound while its nested child violates CDDL.
	if err = l.scanFragment(invalid, &frame); err != nil {
		t.Fatal(err)
	}
	if _, err = l.Encode(nil, frame.Values); err == nil {
		t.Fatal("encoder accepted invalid nested quantity")
	}
	for _, prefix := range []bool{false, true} {
		p, e := PackPAC(l, [][]byte{encoded, encoded}, nil, prefix)
		if e != nil {
			t.Fatal(e)
		}
		parsed, e := ParsePAC(l, p.Bytes, nil)
		if e != nil {
			t.Fatal(e)
		}
		for _, member := range parsed.Members {
			got, e := ResolvePAC(l, parsed.Common, member)
			if e != nil || !bytes.Equal(got, encoded) {
				t.Fatal("PAC common schema resolution", e)
			}
		}
	}
	// Replace a same-length child in a correctly bound PAC. Root framing remains
	// valid; only semantic nested validation can reject this record.
	p, err := PackPAC(l, [][]byte{encoded}, nil, false)
	if err != nil {
		t.Fatal(err)
	}
	raw := append([]byte(nil), p.Bytes...)
	copy(raw[len(raw)-len(invalid):], invalid)
	if _, err = ParsePAC(l, raw, nil); err == nil {
		t.Fatal("PAC accepted invalid child")
	}
	if _, err = l.EncodeJSON([]byte(`{"id":1,"customer":{},"items":[],"kind":"purchase"}`)); err == nil {
		t.Fatal("missing nested required members accepted")
	}
	var doc map[string]RawObject
	if err = UnmarshalObject(l.Bytes, &doc); err != nil {
		t.Fatal(err)
	}
	// Preserve and validate the structured cddl annotation on every registration.
	var fields []Field
	_ = UnmarshalObject(doc["fields"], &fields)
	fields[0].Type = "STRING"
	doc["fields"], _ = MarshalObject(fields)
	altered, _ := MarshalObject(doc)
	if _, err = Parse(altered, IdentityProfile{Primary: -1}); err == nil {
		t.Fatal("accepted mismatched root and schema")
	}
	_ = json.Valid(jsonView)
}
func TestCBORIsNotAPILFieldType(t *testing.T) {
	for _, kind := range []string{"CBOR", "ARRAY<CBOR>"} {
		if _, err := Compile(Definition{Identity: "rejected", Fields: []Field{{Name: "payload", Type: kind}}}); err == nil {
			t.Fatal("accepted", kind)
		}
	}
}

func TestDOCOpenDictionarySharedRuntime(t *testing.T) {
	read := func(name string) []byte {
		t.Helper()
		b, e := os.ReadFile(filepath.Join("..", "..", "rust", "tests", "fixtures", "authoring", name))
		if e != nil {
			t.Fatal(e)
		}
		return b
	}
	l, e := Parse(read("record.rx"), IdentityProfile{Primary: -1})
	if e != nil {
		t.Fatal(e)
	}
	raw, e := l.EncodeJSON(read("record.json"))
	if e != nil || !bytes.Equal(raw, read("record.pil")) {
		t.Fatal("DOC fixture encoding differs", e)
	}
	view, e := l.DecodeJSON(raw)
	if e != nil {
		t.Fatal(e)
	}
	var got, want any
	if e = json.Unmarshal(view, &got); e != nil {
		t.Fatal(e)
	}
	if e = json.Unmarshal(read("record.json"), &want); e != nil {
		t.Fatal(e)
	}
	a, _ := json.Marshal(got)
	b, _ := json.Marshal(want)
	if !bytes.Equal(a, b) {
		t.Fatal("additional fields or dictionary values changed")
	}
	if _, e = l.EncodeJSON([]byte(`{"state":"unseen","count":256}`)); e == nil {
		t.Fatal("range validation bypassed")
	}
}
