package codec

import (
	"crypto/sha256"
	"encoding/binary"
	"fmt"
	pillpacker "github.com/AuthenticityLabs/pill-packer/go"
	"github.com/fxamacker/cbor/v2"
	"strconv"
	"strings"
	"unicode/utf8"
)

const MaxFrame = 16 << 20

type Field struct {
	Name   string   `json:"name" cbor:"name"`
	Type   string   `json:"type" cbor:"type"`
	Values []string `json:"values,omitempty" cbor:"values,omitempty"`
}

type Definition struct {
	PIL              uint32  `json:"pil" cbor:"pil"`
	AdaptiveIntegers string  `json:"adaptive_integers,omitempty" cbor:"adaptive_integers,omitempty"`
	Identity         string  `json:"definition" cbor:"definition"`
	Version          uint64  `json:"version" cbor:"version"`
	Fields           []Field `json:"fields" cbor:"fields"`
}
type IdentityProfile struct {
	Primary  int
	Fields   map[int]bool
	Validate func(Value) error
}
type Layout struct {
	schema     *pillpacker.Definition
	Definition Definition
	Bytes      []byte
	Digest     [32]byte
	Widths     []int
	integers   *integerLayout
	Identity   IdentityProfile
}

var order = binary.LittleEndian
var encoder, _ = cbor.CoreDetEncOptions().EncMode()
var decoder, _ = (cbor.DecOptions{DupMapKey: cbor.DupMapKeyEnforcedAPF, IndefLength: cbor.IndefLengthForbidden, TagsMd: cbor.TagsForbidden, MaxNestedLevels: 32, MaxArrayElements: 65536, MaxMapPairs: 65536}).DecMode()

func FixedStringCapacity(kind string) (int, bool) {
	if !strings.HasPrefix(kind, "STRING(") || !strings.HasSuffix(kind, ")") {
		return 0, false
	}
	n, err := strconv.ParseUint(kind[7:len(kind)-1], 10, 32)
	return int(n), err == nil && n > 0
}
func StringLengthWidth(capacity int) int {
	if capacity <= 255 {
		return 1
	}
	if capacity <= 65535 {
		return 2
	}
	return 4
}
func Width(kind string) (int, error) {
	if n, ok := FixedStringCapacity(kind); ok {
		if n > MaxFrame-4 {
			return 0, fmt.Errorf("STRING slot exceeds frame limit")
		}
		return n + StringLengthWidth(n), nil
	}
	switch kind {
	case "BOOL", "I8", "U8":
		return 1, nil
	case "I16", "U16", "REF16":
		return 2, nil
	case "U24":
		return 3, nil
	case "I32", "U32", "F32":
		return 4, nil
	case "I64", "U64", "F64", "TIMESTAMP_MS", "TIMESTAMP_US", "TIMESTAMP_NS":
		return 8, nil
	case "UUID":
		return 16, nil
	case "STRING", "BYTES", "OBJECT":
		return 0, nil
	}
	if strings.HasPrefix(kind, "ARRAY<") && strings.HasSuffix(kind, ">") {
		element := kind[6 : len(kind)-1]
		if strings.HasPrefix(element, "ARRAY<") {
			return 0, fmt.Errorf("nested arrays unsupported")
		}
		_, err := Width(element)
		return 0, err
	}
	return 0, fmt.Errorf("unsupported PIL type %q", kind)
}

// Parse preserves the complete accepted document, including application annotations.
func Parse(raw []byte, identity IdentityProfile) (*Layout, error) {
	if len(raw) > MaxFrame {
		return nil, fmt.Errorf("definition too large")
	}
	var d Definition
	if err := UnmarshalObject(raw, &d); err != nil {
		return nil, err
	}
	adaptive := d.AdaptiveIntegers != "" && d.AdaptiveIntegers != "none"
	if (d.PIL != 0 && d.PIL != 1) || (d.PIL == 1) != adaptive {
		return nil, fmt.Errorf("adaptive integers require PIL definition version 1; fixed layout requires 0")
	}
	if len(d.Fields) == 0 || len(d.Fields) > 256 {
		return nil, fmt.Errorf("invalid definition field count")
	}
	l := &Layout{Definition: d, Identity: identity}
	byName := map[string]int{}
	for i, f := range d.Fields {
		if f.Name == "" || !utf8.ValidString(f.Name) {
			return nil, fmt.Errorf("invalid field name")
		}
		if _, ok := byName[f.Name]; ok {
			return nil, fmt.Errorf("duplicate field %q", f.Name)
		}
		byName[f.Name] = i
		if f.Name == "@fields" && f.Type != "OBJECT" {
			return nil, fmt.Errorf("unsupported reserved field")
		}
		w, err := Width(f.Type)
		if err != nil {
			return nil, err
		}
		if len(f.Values) > 0 {
			if len(f.Values) > 65536 {
				return nil, fmt.Errorf("dictionary exceeds prototype map limit")
			}
			if f.Type != "STRING" {
				return nil, fmt.Errorf("only STRING may have a dictionary")
			}
			seen := map[string]bool{}
			for _, v := range f.Values {
				if !utf8.ValidString(v) || seen[v] {
					return nil, fmt.Errorf("invalid or duplicate dictionary value")
				}
				seen[v] = true
			}
			w = 1
			if len(f.Values) > 256 {
				w = 2
			}
			if uint64(len(f.Values)) > 65536 {
				w = 4
			}
		}
		l.Widths = append(l.Widths, w)
	}

	if identity.Primary < -1 || identity.Primary >= len(d.Fields) {
		return nil, fmt.Errorf("invalid primary field index")
	}
	var err error
	l.integers, err = compileIntegers(l)
	if err != nil {
		return nil, err
	}
	var annotations map[string]RawObject
	if err := UnmarshalObject(raw, &annotations); err != nil {
		return nil, err
	}
	if _, ok := annotations["cddl"]; ok {
		l.schema, err = pillpacker.FromPIL(raw)
		if err != nil {
			return nil, err
		}
	}
	l.Bytes = append([]byte(nil), raw...)
	l.Digest = sha256.Sum256(raw)
	return l, nil
}
func Compile(d Definition) (*Layout, error) {
	raw, err := MarshalObject(d)
	if err != nil {
		return nil, err
	}
	return Parse(raw, IdentityProfile{Primary: -1})
}
func (l *Layout) Adaptive() bool { return l.integers != nil }
