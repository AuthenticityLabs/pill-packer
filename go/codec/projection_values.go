package codec

import (
	"fmt"
	"github.com/fxamacker/cbor/v2"
	"math"
	"strings"
)

func FieldCBOR(f Field, v Value) ([]byte, error) {
	if v.State == Null {
		return []byte{0xf6}, nil
	}
	if f.Type == "CBOR" {
		return v.Bytes, nil
	}
	if strings.HasPrefix(f.Type, "ARRAY<") {
		element := f.Type[6 : len(f.Type)-1]
		w, e := Width(element)
		if e != nil {
			return nil, e
		}
		at := 0
		n, e := ReadExtended(v.Bytes, &at)
		if e != nil {
			return nil, e
		}
		Ends := make([]uint64, int(n))
		if w > 0 {
			for i := range Ends {
				Ends[i] = uint64((i + 1) * w)
			}
		} else {
			for i := range Ends {
				Ends[i], e = ReadExtended(v.Bytes, &at)
				if e != nil {
					return nil, e
				}
			}
		}
		values := make([]cbor.RawMessage, len(Ends))
		previous := 0
		for i, end := range Ends {
			values[i], e = FieldCBOR(Field{Type: element}, Value{State: Offset, Bytes: v.Bytes[at+previous : at+int(end)]})
			if e != nil {
				return nil, e
			}
			previous = int(end)
		}
		return encoder.Marshal(values)
	}

	if f.Type == "BYTES" || f.Type == "UUID" {
		if v.Bytes == nil {
			return encoder.Marshal([]byte{})
		}
		return encoder.Marshal(v.Bytes)
	}
	if f.Type == "F32" {
		return encoder.Marshal(math.Float32frombits(order.Uint32(v.Bytes)))
	}
	if f.Type == "F64" {
		return encoder.Marshal(math.Float64frombits(order.Uint64(v.Bytes)))
	}
	switch f.Type {
	case "STRING":
		return encoder.Marshal(string(v.Bytes))
	case "BOOL":
		return encoder.Marshal(v.Bytes[0] != 0)
	}
	if strings.HasPrefix(f.Type, "I") || strings.HasPrefix(f.Type, "TIMESTAMP_") {
		n := Unsigned(v.Bytes)
		if len(v.Bytes) < 8 && v.Bytes[len(v.Bytes)-1]&128 != 0 {
			n |= ^uint64(0) << (len(v.Bytes) * 8)
		}
		return encoder.Marshal(int64(n))
	}
	return encoder.Marshal(Unsigned(v.Bytes))
}

// ValueFromCBOR converts one extension value to a declared PIL field. It does
// not decode an entire event or infer a definition. The caller must keep raw
// alive while using the returned value, which may borrow its bytes.
func (l *Layout) ValueFromCBOR(index int, raw []byte) (Value, error) {
	if index < 0 || index >= len(l.Definition.Fields) {
		return Value{}, fmt.Errorf("field index out of range")
	}
	return ValueFromCBOR(l.Definition.Fields[index], l.Widths[index], raw)
}

func ValueFromCBOR(f Field, w int, raw []byte) (Value, error) {
	if len(raw) == 1 && raw[0] == 0xf6 {
		return Value{State: Null}, nil
	}
	if f.Type == "CBOR" {
		return Value{Offset, raw}, nil
	}
	if strings.HasPrefix(f.Type, "ARRAY<") {
		var items []cbor.RawMessage
		if e := decoder.Unmarshal(raw, &items); e != nil {
			return Value{}, e
		}
		element := f.Type[6 : len(f.Type)-1]
		Width, e := Width(element)
		if e != nil {
			return Value{}, e
		}
		body := Extended(nil, uint64(len(items)))
		var data []byte
		for _, item := range items {
			v, e := ValueFromCBOR(Field{Type: element}, Width, item)
			if e != nil {
				return Value{}, e
			}
			if v.State == Null {
				return Value{}, fmt.Errorf("native array elements cannot be null")
			}
			data = append(data, v.Bytes...)
			if Width == 0 {
				body = Extended(body, uint64(len(data)))
			}
		}
		return Value{Offset, append(body, data...)}, nil
	}

	// Reuse the checked scalar decoder for typed extension promotion.
	l := &Layout{Widths: []int{w}, Definition: Definition{Fields: []Field{f}}}
	reader, e := CompilePositionalCBOR(l)
	if e != nil {
		return Value{}, e
	}
	// Dictionary references in @fields are ordinary strings, not numeric IDs.
	if len(f.Values) > 0 {
		var text string
		if e = decoder.Unmarshal(raw, &text); e != nil {
			return Value{}, e
		}
		for i, label := range f.Values {
			if label == text {
				return Uint(uint64(i), w), nil
			}
		}
		return Text(text), nil
	}
	var frame CBORFrame
	record := append([]byte{0xa1, 0}, raw...)
	if e = reader.Scan(record, &frame); e != nil {
		return Value{}, e
	}
	return frame.Values[0], nil
}
