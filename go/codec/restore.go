package codec

import "fmt"

type DirectField struct {
	Index int
	Bytes []byte
}

// RestoreDirect adds sorted, absent direct fields while preserving the encoded
// variable tail. Input must already have passed Scan; no objects are expanded.
func (l *Layout) RestoreDirect(dst, raw []byte, fields []DirectField) ([]byte, error) {
	mask := (len(l.Widths) + 3) / 4
	if len(raw) < mask {
		return nil, fmt.Errorf("short stored frame")
	}
	for i, f := range fields {
		if f.Index < 0 || f.Index >= len(l.Widths) || i > 0 && fields[i-1].Index >= f.Index || !l.ValidDirectWidth(f.Index, f.Bytes) {
			return nil, fmt.Errorf("invalid restored field")
		}
	}
	var plane []byte
	at := mask
	var err error
	if l.integers != nil {
		plane, at, err = l.integers.plane(raw)
		if err != nil {
			return nil, err
		}
	}
	dst = append(dst[:0], raw[:mask]...)
	for _, f := range fields {
		if State(raw[f.Index/4]>>uint(6-2*(f.Index%4))&3) != Absent {
			return nil, fmt.Errorf("restored field shadows an existing value")
		}
		dst[f.Index/4] |= byte(Direct) << uint(6-2*(f.Index%4))
	}
	end := mask
	if l.integers != nil {
		bits := 0
		for i := range l.integers.controls {
			bits += int(l.integers.controls[i][dst[i]])
		}
		end += (bits + 7) / 8
		dst = append(dst, make([]byte, end-mask)...)
	}
	sourceBit, targetBit, next := 0, 0, 0
	for i, w := range l.Widths {
		state := State(raw[i/4] >> uint(6-2*(i%4)) & 3)
		var value []byte
		if next < len(fields) && fields[next].Index == i {
			value = fields[next].Bytes
			next++
		} else if state == Direct {
			if l.integers != nil {
				w, err = l.integers.fieldWidth(i, plane, &sourceBit)
				if err != nil {
					return nil, err
				}
			}
			if w == 0 || w > len(raw)-at {
				return nil, fmt.Errorf("invalid direct span")
			}
			value = raw[at : at+w]
			at += w
		}
		if value == nil {
			continue
		}
		if l.integers != nil {
			f := l.integers.fields[i]
			if f.eligible {
				PutWidthCode(dst[mask:end], targetBit, f.selectorBits, l.integers.code(i, value))
				targetBit += f.selectorBits
			}
		}
		dst = append(dst, value...)
	}
	dst = append(dst, raw[at:]...)
	if len(dst) > MaxFrame {
		return nil, fmt.Errorf("assembled frame too large")
	}
	return dst, nil
}
