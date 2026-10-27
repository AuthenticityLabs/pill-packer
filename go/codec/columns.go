package codec

import (
	"fmt"
	"math"
	"math/bits"
)

// UnsignedEncoding describes fixed-stride residuals from a shared base. It does
// not choose application columns, aggregation functions, or storage windows.
type UnsignedEncoding struct {
	Width int
	Base  uint64
}

func UnsignedWidth(v uint64) int { return max(1, (bits.Len64(v)+7)/8) }
func PutUnsigned(raw []byte, v uint64, c UnsignedEncoding) {
	// Caller validates the descriptor and range once per column.
	v -= c.Base
	switch c.Width {
	case 1:
		raw[0] = byte(v)
	case 2:
		order.PutUint16(raw, uint16(v))
	case 3:
		raw[0], raw[1], raw[2] = byte(v), byte(v>>8), byte(v>>16)
	case 4:
		order.PutUint32(raw, uint32(v))
	case 8:
		order.PutUint64(raw, v)
	default:
		for i := 0; i < c.Width; i++ {
			raw[i] = byte(v)
			v >>= 8
		}
	}
}
func GetUnsigned(raw []byte, c UnsignedEncoding) (uint64, error) {
	if c.Width < 1 || c.Width > 8 || len(raw) < c.Width {
		return 0, fmt.Errorf("invalid unsigned column width")
	}
	var v uint64
	switch c.Width {
	case 1:
		v = uint64(raw[0])
	case 2:
		v = uint64(order.Uint16(raw))
	case 3:
		v = uint64(raw[0]) | uint64(raw[1])<<8 | uint64(raw[2])<<16
	case 4:
		v = uint64(order.Uint32(raw))
	case 8:
		v = order.Uint64(raw)
	default:
		for i := c.Width - 1; i >= 0; i-- {
			v = v<<8 | uint64(raw[i])
		}
	}
	if v > math.MaxUint64-c.Base {
		return 0, fmt.Errorf("unsigned base overflow")
	}
	return c.Base + v, nil
}

// ColumnSpan locates one fixed-width column within an enclosing payload.
type ColumnSpan struct{ Offset, Count, Width int }

// ShuffleColumns groups equal-significance bytes for compression, or reverses
// that transform. Every span is checked before any input bytes are modified.
func ShuffleColumns(raw []byte, spans []ColumnSpan, reverse bool) error {
	end := 0
	for _, c := range spans {
		if c.Offset < end || c.Count < 0 || c.Width < 1 || c.Width > 8 || c.Offset > len(raw) || c.Count > (len(raw)-c.Offset)/c.Width {
			return fmt.Errorf("invalid column span")
		}
		end = c.Offset + c.Count*c.Width
	}
	var scratch []byte
	for _, c := range spans {
		if c.Count < 2 || c.Width < 2 {
			continue
		}
		n := c.Count * c.Width
		if cap(scratch) < n {
			scratch = make([]byte, n)
		} else {
			scratch = scratch[:n]
		}
		copy(scratch, raw[c.Offset:c.Offset+n])
		for b := 0; b < c.Width; b++ {
			for i := 0; i < c.Count; i++ {
				if reverse {
					raw[c.Offset+i*c.Width+b] = scratch[b*c.Count+i]
				} else {
					raw[c.Offset+b*c.Count+i] = scratch[i*c.Width+b]
				}
			}
		}
	}
	return nil
}
