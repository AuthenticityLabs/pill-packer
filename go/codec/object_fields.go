package codec

import (
	"fmt"
	"math"
	"strings"
)

func FieldObject(f Field, v Value) ([]byte, error) {
	value, err := FieldValue(f, v)
	if err != nil {
		return nil, err
	}
	return MarshalObject(value)
}

// FieldValue projects only the requested scalar or nested value.
func FieldValue(f Field, v Value) (any, error) {
	if v.State == Null {
		return nil, nil
	}
	if v.State == Absent {
		return nil, fmt.Errorf("absent field")
	}
	if f.Type == "OBJECT" {
		var value any
		err := UnmarshalObject(v.Bytes, &value)
		return value, err
	}
	if len(f.Values) > 0 && v.State == Direct {
		n := Unsigned(v.Bytes)
		if n >= uint64(len(f.Values)) {
			return nil, fmt.Errorf("invalid dictionary reference")
		}
		return f.Values[n], nil
	}
	if capacity, ok := FixedStringCapacity(f.Type); ok {
		text, err := FixedString(v.Bytes, capacity)
		return string(text), err
	}
	if strings.HasPrefix(f.Type, "ARRAY<") {
		if err := ValidateArray(f.Type, v.Bytes); err != nil {
			return nil, err
		}
		element := f.Type[6 : len(f.Type)-1]
		w, _ := Width(element)
		at := 0
		n, _ := ReadExtended(v.Bytes, &at)
		ends := make([]int, int(n))
		if w > 0 {
			for i := range ends {
				ends[i] = (i + 1) * w
			}
		} else {
			for i := range ends {
				end, err := ReadExtended(v.Bytes, &at)
				if err != nil {
					return nil, err
				}
				ends[i] = int(end)
			}
		}
		values := make([]any, len(ends))
		previous := 0
		for i, end := range ends {
			state := Offset
			if w > 0 {
				state = Direct
			}
			value, err := FieldValue(Field{Type: element}, Value{state, v.Bytes[at+previous : at+end]})
			if err != nil {
				return nil, err
			}
			values[i] = value
			previous = end
		}
		return values, nil
	}
	switch f.Type {
	case "STRING":
		return string(v.Bytes), nil
	case "BOOL":
		if len(v.Bytes) != 1 || v.Bytes[0] > 1 {
			return nil, fmt.Errorf("invalid bool")
		}
		return v.Bytes[0] != 0, nil
	case "UUID", "BYTES":
		return v.Bytes, nil
	case "F32":
		if len(v.Bytes) != 4 {
			return nil, fmt.Errorf("wrong float width")
		}
		return math.Float32frombits(order.Uint32(v.Bytes)), nil
	case "F64":
		if len(v.Bytes) != 8 {
			return nil, fmt.Errorf("wrong float width")
		}
		return math.Float64frombits(order.Uint64(v.Bytes)), nil
	}
	if len(v.Bytes) == 0 || len(v.Bytes) > 8 {
		return nil, fmt.Errorf("invalid integer width")
	}
	n := Unsigned(v.Bytes)
	if strings.HasPrefix(f.Type, "I") || strings.HasPrefix(f.Type, "TIMESTAMP_") {
		if len(v.Bytes) < 8 && v.Bytes[len(v.Bytes)-1]&128 != 0 {
			n |= ^uint64(0) << (len(v.Bytes) * 8)
		}
		return int64(n), nil
	}
	return n, nil
}
func (l *Layout) ValueFromObject(index int, raw []byte) (Value, error) {
	if index < 0 || index >= len(l.Widths) {
		return Value{}, fmt.Errorf("field index out of range")
	}
	return ValueFromObject(l.Definition.Fields[index], l.Widths[index], raw)
}
func ValueFromObject(f Field, w int, raw []byte) (Value, error) {
	node, err := parseObject(raw, 0)
	if err != nil {
		return Value{}, err
	}
	return valueFromNode(f, w, node)
}
func valueFromNode(f Field, w int, node objectNode) (Value, error) {
	if node.kind == 0 {
		return Value{State: Null}, nil
	}
	if f.Type == "OBJECT" {
		return Value{Offset, node.raw}, nil
	}
	if strings.HasPrefix(f.Type, "ARRAY<") {
		if node.kind != 8 {
			return Value{}, fmt.Errorf("expected array")
		}
		element := f.Type[6 : len(f.Type)-1]
		width, err := Width(element)
		if err != nil {
			return Value{}, err
		}
		body := Extended(nil, uint64(len(node.children)))
		var data []byte
		for _, child := range node.children {
			v, err := valueFromNode(Field{Type: element}, width, child)
			if err != nil {
				return Value{}, err
			}
			if v.State == Null {
				return Value{}, fmt.Errorf("null array element")
			}
			data = append(data, v.Bytes...)
			if width == 0 {
				body = Extended(body, uint64(len(data)))
			}
		}
		return Value{Offset, append(body, data...)}, nil
	}
	if capacity, ok := FixedStringCapacity(f.Type); ok {
		text, ok := node.value.(string)
		if !ok {
			return Value{}, fmt.Errorf("expected string")
		}
		return FixedText(text, capacity)
	}
	if f.Type == "STRING" {
		text, ok := node.value.(string)
		if !ok {
			return Value{}, fmt.Errorf("expected text")
		}
		for i, s := range f.Values {
			if text == s {
				return Uint(uint64(i), w), nil
			}
		}
		return Text(text), nil
	}
	if f.Type == "BYTES" || f.Type == "UUID" {
		b, ok := node.value.([]byte)
		if !ok || f.Type == "UUID" && len(b) != 16 {
			return Value{}, fmt.Errorf("expected bytes of declared width")
		}
		state := Offset
		if f.Type == "UUID" {
			state = Direct
		}
		return Value{state, b}, nil
	}
	if f.Type == "BOOL" {
		b, ok := node.value.(bool)
		if !ok {
			return Value{}, fmt.Errorf("expected bool")
		}
		if b {
			return Uint(1, 1), nil
		}
		return Uint(0, 1), nil
	}
	if f.Type == "F32" || f.Type == "F64" {
		n, ok := node.value.(float64)
		if !ok {
			return Value{}, fmt.Errorf("expected float")
		}
		if f.Type == "F32" {
			if math.IsInf(float64(float32(n)), 0) && !math.IsInf(n, 0) {
				return Value{}, fmt.Errorf("F32 overflow")
			}
			return Uint(uint64(math.Float32bits(float32(n))), 4), nil
		}
		return Uint(math.Float64bits(n), 8), nil
	}
	if w < 1 || w > 8 {
		return Value{}, fmt.Errorf("unsupported scalar type")
	}
	signed := strings.HasPrefix(f.Type, "I") || strings.HasPrefix(f.Type, "TIMESTAMP_")
	if signed {
		var n int64
		switch x := node.value.(type) {
		case int64:
			n = x
		case uint64:
			if x > math.MaxInt64 {
				return Value{}, fmt.Errorf("integer overflow")
			}
			n = int64(x)
		default:
			return Value{}, fmt.Errorf("expected integer")
		}
		if w < 8 && (n < -(int64(1)<<(w*8-1)) || n >= (int64(1)<<(w*8-1))) {
			return Value{}, fmt.Errorf("integer overflow")
		}
		return Uint(uint64(n), w), nil
	}
	n, ok := node.value.(uint64)
	if !ok || w < 8 && n >= (uint64(1)<<(w*8)) {
		return Value{}, fmt.Errorf("unsigned integer overflow")
	}
	return Uint(n, w), nil
}
