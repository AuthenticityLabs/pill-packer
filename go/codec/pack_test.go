package codec

import (
	"bytes"
	"testing"
)

func TestPackAddressingAndHeaders(t *testing.T) {
	def, err := Compile(Definition{PIL: 0, Identity: "pack-test", Fields: []Field{{Name: "n", Type: "U16"}, {Name: "s", Type: "STRING"}}})
	if err != nil {
		t.Fatal(err)
	}
	a, err := Fill(def, []Value{Uint(1, 2), Text("same")})
	if err != nil {
		t.Fatal(err)
	}
	b, _ := Fill(def, []Value{Uint(2, 2), Text("same")})
	for _, width := range []int{0, 2, 3, 4} {
		for _, common := range []bool{false, true} {
			p, err := Pack(def, [][]byte{a, b}, PackOptions{AddressBytes: width, CommonValues: common, Headers: map[string][]byte{"Producer": []byte("test")}})
			if err != nil {
				t.Fatal(err)
			}
			parsed, err := ParsePAC(def, p.Bytes, nil)
			if err != nil {
				t.Fatal(err)
			}
			for i, want := range [][]byte{a, b} {
				got, err := parsed.Get(def, i)
				if err != nil || !bytes.Equal(got, want) {
					t.Fatalf("get %d: %x %v", i, got, err)
				}
			}
			if !bytes.Equal(parsed.Headers["Producer"], []byte("test")) {
				t.Fatal("header")
			}
			compressed, err := CompressPAC(p.Bytes, 1)
			if err != nil {
				t.Fatal(err)
			}
			if order.Uint32(compressed[4:]) != PACGzip {
				t.Fatal("requested codec ignored")
			}
			expanded, err := ExpandPAC(compressed)
			if err != nil || !bytes.Equal(expanded, p.Bytes) {
				t.Fatal(err)
			}
		}
	}
	p, err := Pack(def, [][]byte{a, b}, PackOptions{Fixed: true})
	if err != nil || p.RecordWidth != uint32(len(a)) {
		t.Fatal(err)
	}
	if _, err := Pack(def, [][]byte{a, b}, PackOptions{Fixed: true, RecordWidth: 1}); err == nil {
		t.Fatal("wrong width accepted")
	}
	if _, err := Pack(def, [][]byte{a, b}, PackOptions{AddressBytes: 2, Offsets: make([]byte, 4)}); err == nil {
		t.Fatal("wrong directory accepted")
	}
}
