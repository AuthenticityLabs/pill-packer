package codec

import (
	"fmt"
	"io"
)

// Write emits a PIL frame while borrowing variable payloads. Large byte fields
// are written directly; only the mask, scalars and offsets need scratch space.
func (l *Layout) Write(out io.Writer, values []Value) error {
	if l.integers != nil {
		raw, err := l.Encode(nil, values)
		if err != nil {
			return err
		}
		return writeFull(out, raw)
	}
	if len(values) != len(l.Widths) {
		return fmt.Errorf("field count mismatch")
	}
	var scratch [256]byte
	head := append(scratch[:0], make([]byte, (len(values)+3)/4)...)
	for i, v := range values {
		if v.State > Offset {
			return fmt.Errorf("invalid state")
		}
		head[i/4] |= byte(v.State) << uint(6-2*(i%4))
		if v.State == Direct {
			if len(v.Bytes) != l.Widths[i] || l.Widths[i] == 0 {
				return fmt.Errorf("direct width mismatch")
			}
			head = append(head, v.Bytes...)
		}
	}
	var end uint64
	for _, v := range values {
		if v.State == Offset {
			end += uint64(len(v.Bytes))
			if end > MaxFrame {
				return fmt.Errorf("frame too large")
			}
			head = Extended(head, end)
		}
	}
	if uint64(len(head))+end > MaxFrame {
		return fmt.Errorf("frame too large")
	}
	if err := writeFull(out, head); err != nil {
		return err
	}
	for _, v := range values {
		if v.State == Offset {
			if err := writeFull(out, v.Bytes); err != nil {
				return err
			}
		}
	}
	return nil
}
func writeFull(out io.Writer, raw []byte) error {
	n, err := out.Write(raw)
	if err == nil && n != len(raw) {
		return io.ErrShortWrite
	}
	return err
}
