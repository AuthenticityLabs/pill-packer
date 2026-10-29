package codec

import (
	"bytes"
	"testing"
)

func TestFixedStringSlots(t *testing.T) {
	for _, mode := range []string{"", "16"} {
		def := Definition{Identity: "fixed", Fields: []Field{{Name: "text", Type: "STRING(4)"}}, AdaptiveIntegers: mode}
		if mode != "" {
			def.PIL = 1
		}
		l, err := Compile(def)
		if err != nil {
			t.Fatal(err)
		}
		v, err := FixedText("a\x00 ", 4)
		if err != nil {
			t.Fatal(err)
		}
		b, err := l.Encode(nil, []Value{v})
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(b, []byte{0x80, 3, 'a', 0, ' ', 0}) {
			t.Fatalf("%x", b)
		}
		var frame Frame
		if err = l.Scan(b, &frame); err != nil {
			t.Fatal(err)
		}
		text, err := FixedString(frame.Values[0].Bytes, 4)
		if err != nil || string(text) != "a\x00 " {
			t.Fatal(text, err)
		}
		b[len(b)-1] = 1
		if l.Scan(b, &frame) == nil {
			t.Fatal("accepted nonzero padding")
		}
	}
	for _, capacity := range []int{255, 256, 65535, 65536} {
		v, err := FixedText("é", capacity)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = FixedString(v.Bytes, capacity); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := FixedText("ééé", 4); err == nil {
		t.Fatal("accepted overflow")
	}
}
