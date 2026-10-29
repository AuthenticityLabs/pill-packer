package codec

// Native PIL bootstrap values. Definitions and open object attributes share this
// codec; storage policy and transport annotations remain application concerns.
import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

type RawObject []byte

var objectLayout = &Layout{
	Definition: Definition{Identity: "pil/value", Fields: []Field{{Name: "kind", Type: "U8"}, {Name: "unsigned", Type: "U64"}, {Name: "negative", Type: "I64"}, {Name: "float", Type: "F64"}, {Name: "text", Type: "STRING"}, {Name: "bytes", Type: "BYTES"}, {Name: "children", Type: "ARRAY<BYTES>"}}},
	Widths:     []int{1, 8, 8, 8, 0, 0, 0},
}

func MarshalObject(value any) ([]byte, error) { return marshalObject(reflect.ValueOf(value), 0) }
func marshalObject(v reflect.Value, depth int) ([]byte, error) {
	if depth > 64 {
		return nil, fmt.Errorf("PIL value nesting limit")
	}
	fields := make([]Value, 7)
	kind := uint64(0)
	for v.IsValid() && (v.Kind() == reflect.Interface || v.Kind() == reflect.Pointer) {
		if v.IsNil() {
			v = reflect.Value{}
			break
		}
		v = v.Elem()
	}
	var children [][]byte
	if v.IsValid() {
		if v.CanInterface() {
			if custom, ok := v.Interface().(interface{ PILValue() any }); ok {
				return marshalObject(reflect.ValueOf(custom.PILValue()), depth+1)
			}
		}
		if v.Type() == reflect.TypeOf(RawObject{}) {
			b := v.Bytes()
			if _, err := parseObject(b, depth); err != nil {
				return nil, err
			}
			return append([]byte(nil), b...), nil
		}
		if v.Type() == reflect.TypeOf(time.Time{}) {
			return marshalObject(reflect.ValueOf(v.Interface().(time.Time).Format(time.RFC3339Nano)), depth)
		}
		if v.Type() == reflect.TypeOf(json.Number("")) {
			n := v.Interface().(json.Number)
			if i, err := n.Int64(); err == nil {
				return marshalObject(reflect.ValueOf(i), depth)
			}
			if u, err := strconv.ParseUint(string(n), 10, 64); err == nil {
				return marshalObject(reflect.ValueOf(u), depth)
			}
			if !strings.ContainsAny(string(n), ".eE") {
				return nil, fmt.Errorf("JSON integer exceeds PIL I64/U64 range")
			}
			f, err := n.Float64()
			if err != nil {
				return nil, err
			}
			return marshalObject(reflect.ValueOf(f), depth)
		}
		switch v.Kind() {
		case reflect.Bool:
			kind = 1
			if v.Bool() {
				kind = 2
			}
		case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
			kind = 3
			fields[1] = Uint(v.Uint(), 8)
		case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
			n := v.Int()
			if n >= 0 {
				kind = 3
				fields[1] = Uint(uint64(n), 8)
			} else {
				kind = 4
				fields[2] = Uint(uint64(n), 8)
			}
		case reflect.Float32, reflect.Float64:
			kind = 5
			fields[3] = Uint(math.Float64bits(v.Float()), 8)
		case reflect.String:
			if !utf8.ValidString(v.String()) {
				return nil, fmt.Errorf("invalid PIL UTF-8")
			}
			kind = 6
			fields[4] = Text(v.String())
		case reflect.Array, reflect.Slice:
			if v.Type().Elem().Kind() == reflect.Uint8 {
				kind = 7
				data := make([]byte, v.Len())
				reflect.Copy(reflect.ValueOf(data), v)
				fields[5] = Value{Offset, data}
			} else {
				kind = 8
				for i := 0; i < v.Len(); i++ {
					b, err := marshalObject(v.Index(i), depth+1)
					if err != nil {
						return nil, err
					}
					children = append(children, b)
				}
			}
		case reflect.Map, reflect.Struct:
			kind = 9
			type pair struct{ k, v []byte }
			var pairs []pair
			add := func(k, value reflect.Value) error {
				if k.Kind() != reflect.String && !(k.Kind() >= reflect.Int && k.Kind() <= reflect.Uint64) {
					return fmt.Errorf("PIL object key must be text or integer")
				}
				kb, err := marshalObject(k, depth+1)
				if err != nil {
					return err
				}
				vb, err := marshalObject(value, depth+1)
				if err != nil {
					return err
				}
				pairs = append(pairs, pair{kb, vb})
				return nil
			}
			if v.Kind() == reflect.Map {
				it := v.MapRange()
				for it.Next() {
					key := it.Key()
					for key.Kind() == reflect.Interface {
						key = key.Elem()
					}
					if err := add(key, it.Value()); err != nil {
						return nil, err
					}
				}
			} else {
				for i := 0; i < v.NumField(); i++ {
					f := v.Type().Field(i)
					if !f.IsExported() {
						continue
					}
					name, omit := objectFieldName(f)
					if name == "-" {
						continue
					}
					fv := v.Field(i)
					if omit && fv.IsZero() {
						continue
					}
					key := reflect.ValueOf(name)
					if strings.Contains(f.Tag.Get("pil"), "keyasint") {
						n, err := strconv.ParseUint(name, 10, 64)
						if err != nil {
							return nil, err
						}
						key = reflect.ValueOf(n)
					}
					if err := add(key, fv); err != nil {
						return nil, err
					}
				}
			}
			sort.Slice(pairs, func(i, j int) bool { return bytes.Compare(pairs[i].k, pairs[j].k) < 0 })
			for i, p := range pairs {
				if i > 0 && bytes.Equal(pairs[i-1].k, p.k) {
					return nil, fmt.Errorf("duplicate PIL object key")
				}
				children = append(children, p.k, p.v)
			}
		default:
			return nil, fmt.Errorf("unsupported PIL value %s", v.Type())
		}
	}
	fields[0] = Uint(kind, 1)
	if kind >= 8 {
		body := Extended(nil, uint64(len(children)))
		size := 0
		for _, child := range children {
			if len(child) > MaxFrame-size {
				return nil, fmt.Errorf("PIL value exceeds byte limit")
			}
			size += len(child)
			body = Extended(body, uint64(size))
		}
		for _, child := range children {
			body = append(body, child...)
		}
		fields[6] = Value{Offset, body}
	}
	return objectLayout.Encode(nil, fields)
}
func objectFieldName(field reflect.StructField) (string, bool) {
	tag := field.Tag.Get("pil")
	if tag == "" {
		tag = field.Tag.Get("json")
	}
	if tag == "" {
		tag = field.Tag.Get("cbor")
	}
	parts := strings.Split(tag, ",")
	name := parts[0]
	if name == "" {
		name = field.Name
	}
	omit := false
	for _, part := range parts[1:] {
		omit = omit || part == "omitempty"
	}
	return name, omit
}

type objectNode struct {
	raw      []byte
	value    any
	children []objectNode
	kind     byte
}

func parseObject(raw []byte, depth int) (objectNode, error) {
	fail := func() (objectNode, error) { return objectNode{}, fmt.Errorf("invalid PIL value") }
	if depth > 64 || len(raw) > MaxFrame {
		return fail()
	}
	var frame Frame
	if err := objectLayout.Scan(raw, &frame); err != nil {
		return objectNode{}, err
	}
	f := frame.Values
	if f[0].State != Direct {
		return fail()
	}
	kind := f[0].Bytes[0]
	active := 0
	switch kind {
	case 0, 1, 2:
	case 3:
		active = 1
	case 4:
		active = 2
	case 5:
		active = 3
	case 6:
		active = 4
	case 7:
		active = 5
	case 8, 9:
		active = 6
	default:
		return fail()
	}
	for i := 1; i < 7; i++ {
		if i != active && f[i].State != Absent {
			return fail()
		}
	}
	node := objectNode{raw: raw, kind: kind}
	if active > 0 && f[active].State < Direct {
		return fail()
	}
	switch kind {
	case 0:
		node.value = nil
	case 1:
		node.value = false
	case 2:
		node.value = true
	case 3:
		node.value = Unsigned(f[1].Bytes)
	case 4:
		n := int64(Unsigned(f[2].Bytes))
		if n >= 0 {
			return fail()
		}
		node.value = n
	case 5:
		node.value = math.Float64frombits(Unsigned(f[3].Bytes))
	case 6:
		node.value = string(f[4].Bytes)
	case 7:
		node.value = f[5].Bytes
	case 8, 9:
		b := f[6].Bytes
		at := 0
		n, err := ReadExtended(b, &at)
		if err != nil {
			return objectNode{}, err
		}
		if n > uint64(len(b)/2) {
			return fail()
		}
		ends := make([]int, int(n))
		for i := range ends {
			end, err := ReadExtended(b, &at)
			if err != nil || end > uint64(len(b)) {
				return fail()
			}
			ends[i] = int(end)
		}
		prev := 0
		seen := map[string]bool{}
		for i, end := range ends {
			if end < prev || end > len(b)-at {
				return fail()
			}
			child, err := parseObject(b[at+prev:at+end], depth+1)
			if err != nil {
				return objectNode{}, err
			}
			if kind == 9 && i%2 == 0 {
				if child.kind != 3 && child.kind != 4 && child.kind != 6 {
					return fail()
				}
				key, err := MarshalObject(child.value)
				if err != nil {
					return objectNode{}, err
				}
				if seen[string(key)] {
					return fail()
				}
				seen[string(key)] = true
			}
			node.children = append(node.children, child)
			prev = end
		}
		if kind == 9 && n%2 != 0 {
			return fail()
		}
	}
	return node, nil
}
func UnmarshalObject(raw []byte, target any) error {
	node, err := parseObject(raw, 0)
	if err != nil {
		return err
	}
	v := reflect.ValueOf(target)
	if v.Kind() != reflect.Pointer || v.IsNil() {
		return fmt.Errorf("PIL target must be a nonnil pointer")
	}
	return assignObject(node, v.Elem())
}
func (n objectNode) any() any {
	switch n.kind {
	case 8:
		a := make([]any, len(n.children))
		for i, c := range n.children {
			a[i] = c.any()
		}
		return a
	case 9:
		m := make(map[any]any, len(n.children)/2)
		for i := 0; i < len(n.children); i += 2 {
			m[n.children[i].value] = n.children[i+1].any()
		}
		return m
	default:
		return n.value
	}
}
func assignObject(n objectNode, v reflect.Value) error {
	if v.Type() == reflect.TypeOf(RawObject{}) {
		v.SetBytes(append([]byte(nil), n.raw...))
		return nil
	}
	if v.Type() == reflect.TypeOf(time.Time{}) {
		text, ok := n.value.(string)
		if !ok {
			return fmt.Errorf("PIL time must be RFC3339 text")
		}
		t, err := time.Parse(time.RFC3339Nano, text)
		if err != nil {
			return err
		}
		v.Set(reflect.ValueOf(t))
		return nil
	}
	if n.kind == 0 {
		v.SetZero()
		return nil
	}
	if v.Kind() == reflect.Pointer {
		v.Set(reflect.New(v.Type().Elem()))
		return assignObject(n, v.Elem())
	}
	if v.Kind() == reflect.Interface {
		v.Set(reflect.ValueOf(n.any()))
		return nil
	}
	mismatch := func() error { return fmt.Errorf("PIL value does not fit %s", v.Type()) }
	switch v.Kind() {
	case reflect.Struct:
		if n.kind != 9 {
			return mismatch()
		}
		fields := map[string]int{}
		for i := 0; i < v.NumField(); i++ {
			f := v.Type().Field(i)
			if f.IsExported() {
				name, _ := objectFieldName(f)
				fields[name] = i
			}
		}
		for i := 0; i < len(n.children); i += 2 {
			key, ok := n.children[i].value.(string)
			if !ok {
				if n, ok := n.children[i].value.(uint64); ok {
					key = strconv.FormatUint(n, 10)
				} else {
					return mismatch()
				}
			}
			if j, ok := fields[key]; ok {
				if err := assignObject(n.children[i+1], v.Field(j)); err != nil {
					return err
				}
			}
		}
		return nil
	case reflect.Map:
		if n.kind != 9 {
			return mismatch()
		}
		v.Set(reflect.MakeMapWithSize(v.Type(), len(n.children)/2))
		for i := 0; i < len(n.children); i += 2 {
			k := reflect.New(v.Type().Key()).Elem()
			x := reflect.New(v.Type().Elem()).Elem()
			if err := assignObject(n.children[i], k); err != nil {
				return err
			}
			if err := assignObject(n.children[i+1], x); err != nil {
				return err
			}
			v.SetMapIndex(k, x)
		}
		return nil
	case reflect.Slice, reflect.Array:
		if v.Type().Elem().Kind() == reflect.Uint8 && n.kind == 7 {
			b := n.value.([]byte)
			if v.Kind() == reflect.Slice {
				v.SetBytes(append([]byte{}, b...))
				return nil
			}
			if v.Len() != len(b) {
				return mismatch()
			}
			reflect.Copy(v, reflect.ValueOf(b))
			return nil
		}
		if n.kind != 8 {
			return mismatch()
		}
		if v.Kind() == reflect.Slice {
			v.Set(reflect.MakeSlice(v.Type(), len(n.children), len(n.children)))
		} else if v.Len() != len(n.children) {
			return mismatch()
		}
		for i, c := range n.children {
			if err := assignObject(c, v.Index(i)); err != nil {
				return err
			}
		}
		return nil
	case reflect.Bool:
		if b, ok := n.value.(bool); ok {
			v.SetBool(b)
			return nil
		}
	case reflect.String:
		if s, ok := n.value.(string); ok {
			v.SetString(s)
			return nil
		}
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		if u, ok := n.value.(uint64); ok && !v.OverflowUint(u) {
			v.SetUint(u)
			return nil
		}
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		if x, ok := n.value.(int64); ok && !v.OverflowInt(x) {
			v.SetInt(x)
			return nil
		}
		if x, ok := n.value.(uint64); ok && x <= math.MaxInt64 && !v.OverflowInt(int64(x)) {
			v.SetInt(int64(x))
			return nil
		}
	case reflect.Float32, reflect.Float64:
		if x, ok := n.value.(float64); ok && !v.OverflowFloat(x) {
			v.SetFloat(x)
			return nil
		}
	}
	return mismatch()
}
