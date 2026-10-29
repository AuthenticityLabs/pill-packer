package codec

import (
	"bytes"
	"encoding/hex"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestNativeObjectInteropAndValidation(t *testing.T) {
	value := map[any]any{
		"max": uint64(math.MaxUint64), "min": int64(math.MinInt64),
		"text": "é\x00 ", "bytes": []byte{0, 255}, "empty": []any{},
		"nested": map[any]any{uint64(7): []any{nil, false, true, float64(1.5)}},
	}
	raw, err := MarshalObject(value)
	if err != nil {
		t.Fatal(err)
	}
	var decoded any
	if err = UnmarshalObject(raw, &decoded); err != nil || !reflect.DeepEqual(value, decoded) {
		t.Fatal(decoded, err)
	}
	path := filepath.Join("..", "..", "npm", "fixtures", "object.hex")
	if os.Getenv("PIL_UPDATE_FIXTURES") == "1" {
		if err = os.WriteFile(path, []byte(hex.EncodeToString(raw)+"\n"), 0644); err != nil {
			t.Fatal(err)
		}
	}
	fixture, err := os.ReadFile(path)
	if err != nil || string(bytes.TrimSpace(fixture)) != hex.EncodeToString(raw) {
		t.Fatal("object fixture differs", err)
	}
	for n := 0; n < len(raw); n++ {
		if UnmarshalObject(raw[:n], &decoded) == nil {
			t.Fatalf("accepted truncation %d", n)
		}
	}
	if _, err = MarshalObject(map[any]any{uint64(1): true, int64(1): false}); err == nil {
		t.Fatal("duplicate integer key")
	}
	l, err := Compile(Definition{Identity: "object/example", Fields: []Field{{Name: "payload", Type: "OBJECT"}, {Name: "@fields", Type: "OBJECT"}}})
	if err != nil {
		t.Fatal(err)
	}
	extra, _ := MarshalObject(map[string]any{"new": value})
	encoded, err := l.Encode(nil, []Value{{Offset, raw}, {Offset, extra}})
	if err != nil {
		t.Fatal(err)
	}
	var frame Frame
	if err = l.Scan(encoded, &frame); err != nil {
		t.Fatal(err)
	}
	for _, bad := range []any{nil, []any{}, map[string]any{"payload": true}, map[string]any{"@reserved": true}, map[any]any{uint64(1): true}} {
		extra, _ := MarshalObject(bad)
		if l.ValidateOffset(1, extra) == nil {
			t.Fatalf("accepted invalid extension: %#v", bad)
		}
	}
	cbor, err := FieldCBOR(Field{Type: "OBJECT"}, frame.Values[0])
	if err != nil {
		t.Fatal(err)
	}
	back, err := ValueFromCBOR(Field{Type: "OBJECT"}, 0, cbor)
	if err != nil || !bytes.Equal(back.Bytes, raw) {
		t.Fatal("ingress object conversion", err)
	}
}

func TestBootstrapDefinitionRXReadsItsOwnEncoding(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("..", "..", "schemas", "pil.rx"))
	if err != nil {
		t.Fatal(err)
	}
	l, err := Parse(raw, IdentityProfile{Primary: -1})
	if err != nil {
		t.Fatal(err)
	}
	if l.Definition.Identity != "pil/value" || len(l.Widths) != 7 {
		t.Fatal("wrong bootstrap layout")
	}
	var frame Frame
	if err = l.Scan(raw, &frame); err != nil {
		t.Fatal(err)
	}
	if frame.Values[0].State != Direct || frame.Values[0].Bytes[0] != 9 {
		t.Fatal("bootstrap is not its own encoded map")
	}
}
