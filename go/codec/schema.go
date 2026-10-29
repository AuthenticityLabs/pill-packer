package codec

import "fmt"

// EncodeJSON and DecodeJSON use the shared schema interpreter. This keeps named
// rules, nested frames, sparse members, choices, and constraints identical in Go,
// the CLI, and WASM. Ordinary layouts keep their allocation-free Go scanner.
func (l *Layout) EncodeJSON(raw []byte) ([]byte, error) {
	if l.schema == nil {
		return nil, fmt.Errorf("definition has no CDDL schema")
	}
	return l.schema.EncodeJSON(raw)
}
func (l *Layout) DecodeJSON(raw []byte) ([]byte, error) {
	if l.schema == nil {
		return nil, fmt.Errorf("definition has no CDDL schema")
	}
	return l.schema.DecodeJSON(raw)
}
