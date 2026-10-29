package codec

import "fmt"

// PackOptions selects representation only; grouping and storage belong to callers.
type PackOptions struct {
	CommonValues bool
	Headers      map[string][]byte
	AddressBytes int
	Offsets      []byte
	Fixed        bool
	RecordWidth  uint32
}

func Define(rx []byte) (*Layout, error)                { return Parse(rx, IdentityProfile{Primary: -1}) }
func Fill(def *Layout, fields []Value) ([]byte, error) { return def.Encode(nil, fields) }
func Pack(def *Layout, records [][]byte, options PackOptions) (*PAC, error) {
	p, err := FactorPAC(def, records, nil, options.CommonValues)
	if err != nil {
		return nil, err
	}
	p.Headers = options.Headers
	p.AddressBytes = options.AddressBytes
	p.Directory = options.Offsets
	if options.Fixed {
		p.RecordWidth = options.RecordWidth
		if p.RecordWidth == 0 && len(p.Members) > 0 {
			p.RecordWidth = uint32(len(p.Members[0]))
		}
		if p.RecordWidth == 0 {
			return nil, fmt.Errorf("empty fixed PAC requires width")
		}
	} else if options.RecordWidth != 0 {
		return nil, fmt.Errorf("record width requires fixed layout")
	}
	p.Bytes, err = p.Marshal()
	return p, err
}
func (p *PAC) Get(def *Layout, ordinal int) ([]byte, error) {
	if ordinal < 0 || ordinal >= len(p.Members) {
		return nil, fmt.Errorf("PAC ordinal outside pack")
	}
	return ResolvePAC(def, p.Common, p.Members[ordinal])
}
func (p *PAC) GetOffset(def *Layout, offset uint64) ([]byte, error) {
	var at uint64
	for i, m := range p.Members {
		if at == offset {
			return p.Get(def, i)
		}
		at += uint64(len(m))
	}
	return nil, fmt.Errorf("offset is not a member boundary")
}
func Unpack(def *Layout, p *PAC) ([][]byte, map[string][]byte, error) {
	records := make([][]byte, len(p.Members))
	for i := range records {
		raw, err := p.Get(def, i)
		if err != nil {
			return nil, nil, err
		}
		records[i] = raw
	}
	return records, p.Headers, nil
}
func Push(def *Layout, p *PAC, record []byte, options PackOptions) (*PAC, error) {
	records, _, err := Unpack(def, p)
	if err != nil {
		return nil, err
	}
	if options.Headers == nil {
		options.Headers = p.Headers
	}
	return Pack(def, append(records, record), options)
}
func Pop(def *Layout, p *PAC, options PackOptions) (*PAC, []byte, error) {
	records, _, err := Unpack(def, p)
	if err != nil {
		return nil, nil, err
	}
	var last []byte
	if len(records) > 0 {
		last = records[len(records)-1]
		records = records[:len(records)-1]
	}
	if options.Headers == nil {
		options.Headers = p.Headers
	}
	out, err := Pack(def, records, options)
	return out, last, err
}
