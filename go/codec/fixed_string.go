package codec

import (
	"fmt"
	"unicode/utf8"
)

// FixedText encodes text without trimming, truncation, or UTF-8 normalization.
func FixedText(text string, capacity int) (Value, error) {
	if capacity < 1 || capacity > MaxFrame-4 || len(text) > capacity || !utf8.ValidString(text) {
		return Value{}, fmt.Errorf("invalid fixed STRING capacity or UTF-8 payload")
	}
	prefix := StringLengthWidth(capacity)
	data := make([]byte, prefix+capacity)
	n := uint32(len(text))
	for i := 0; i < prefix; i++ {
		data[i] = byte(n >> (8 * i))
	}
	copy(data[prefix:], text)
	return Value{Direct, data}, nil
}

// FixedString borrows the used portion of a validated fixed-capacity slot.
func FixedString(raw []byte, capacity int) ([]byte, error) {
	prefix := StringLengthWidth(capacity)
	if capacity < 1 || len(raw) != prefix+capacity {
		return nil, fmt.Errorf("wrong fixed STRING width")
	}
	n := Unsigned(raw[:prefix])
	if n > uint64(capacity) {
		return nil, fmt.Errorf("STRING length exceeds capacity")
	}
	end := prefix + int(n)
	for _, b := range raw[end:] {
		if b != 0 {
			return nil, fmt.Errorf("nonzero STRING padding")
		}
	}
	if !utf8.Valid(raw[prefix:end]) {
		return nil, fmt.Errorf("invalid STRING UTF-8")
	}
	return raw[prefix:end], nil
}
