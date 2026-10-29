package codec

// PAC is a version-zero representation, not a transport/session binding.
// Compression wraps the complete decoded PAC, including its semantic header.
import (
	"bytes"
	"fmt"
	"io"
	"math"
	"sort"
	"unicode/utf8"
)

const PACHeaderSize = 64
const PACCompressedHeaderSize = 16
const PACCommon = uint32(1)
const PACGzip = uint32(2)
const PACSource = uint32(1) // optional UTF-8 source name, scoped by definition digest
const PACMaximum = math.MaxUint32
const PACMaximumMembers = math.MaxUint32

type PACExtension struct {
	Type     uint32
	Required bool
	Data     []byte
}

type PAC struct {
	Digest       [32]byte
	Common       []byte
	Members      [][]byte
	Extensions   []PACExtension
	Bytes        []byte
	Headers      map[string][]byte
	AddressBytes int
	RecordWidth  uint32
	Directory    []byte
}

// Members remain variable length: field types do not require every field to be
// present. Preserve ABSENT/NULL states without fabricated payload slots.

// PackPAC factors only equal, present positional values. It does not construct
// semantic maps, reorder records, or change ABSENT/NULL semantics.
func PackPAC(l *Layout, frames [][]byte, extensions []PACExtension, prefix bool) (*PAC, error) {
	p, err := FactorPAC(l, frames, extensions, prefix)
	if err != nil {
		return nil, err
	}
	p.Bytes, err = p.Marshal()
	return p, err
}

// Storage assigns native placement after factoring, then serializes exactly once.
func FactorPAC(l *Layout, frames [][]byte, extensions []PACExtension, prefix bool) (*PAC, error) {
	if len(frames) > PACMaximumMembers {
		return nil, fmt.Errorf("PAC member limit")
	}
	var common []Value
	var active []int
	var scan Frame
	total := 0
	for i, raw := range frames {
		if err := l.ScanPlain(raw, &scan); err != nil {
			return nil, err
		}
		if len(raw) > PACMaximum-total {
			return nil, fmt.Errorf("PAC exceeds byte limit")
		}
		total += len(raw)
		if prefix && len(frames) > 1 {
			if i == 0 {
				common = append([]Value(nil), scan.Values...)
				active = make([]int, 0, len(common))
				for f, v := range common {
					if v.State != Absent {
						active = append(active, f)
					}
				}
			} else {
				for i := 0; i < len(active); {
					f := active[i]
					v := common[f]
					if v.State != scan.Values[f].State || !bytes.Equal(v.Bytes, scan.Values[f].Bytes) {
						common[f] = Value{}
						active[i] = active[len(active)-1]
						active = active[:len(active)-1]
					} else {
						i++
					}
				}
			}
		}
	}
	p := &PAC{Digest: l.Digest, Members: frames, Extensions: extensions}
	if len(active) > 0 {
		var err error
		p.Common, err = l.encodeFragment(nil, common)
		if err != nil {
			return nil, err
		}
		packed := make([][]byte, len(frames))
		arena := make([]byte, 0, total)
		var scratch []byte
		size := len(p.Common)
		for i, raw := range frames {
			if err = l.scanFragment(raw, &scan); err != nil {
				return nil, err
			}
			for f, v := range common {
				if v.State != Absent {
					scan.Values[f] = Value{}
				}
			}
			scratch, err = l.encodeFragment(scratch, scan.Values)
			if err != nil {
				return nil, err
			}
			start := len(arena)
			arena = append(arena, scratch...)
			packed[i] = arena[start:len(arena)]
			size += len(packed[i])
		}
		// Include the variable common-length prefix in the decision.
		if size+len(Extended(nil, uint64(len(p.Common)))) < total+2 {
			p.Members = packed
		} else {
			p.Common = nil
		}
	}
	return p, nil
}

func (p *PAC) Marshal() ([]byte, error) {
	h := make([]byte, PACHeaderSize)
	order.PutUint32(h[4:8], 4)

	if len(p.Common) > 0 {
		order.PutUint32(h[4:8], PACCommon|4)
	}
	order.PutUint64(h[24:32], uint64(len(p.Members)))
	copy(h[32:64], p.Digest[:])
	h = Extended(h, uint64(len(p.Extensions)))
	seen := map[uint32]bool{}
	for _, e := range p.Extensions {
		if seen[e.Type] {
			return nil, fmt.Errorf("duplicate PAC extension")
		}
		seen[e.Type] = true
		if len(e.Data) > PACMaximum-len(h)-22 {
			return nil, fmt.Errorf("PAC extension exceeds limit")
		}
		h = order.AppendUint32(h, e.Type)
		h = order.AppendUint16(h, 0) // schema version zero
		var flags uint16
		if e.Required {
			flags = 1
		}
		h = order.AppendUint16(h, flags)
		h = Extended(h, uint64(len(e.Data)))
		h = append(h, e.Data...)
	}
	if p.AddressBytes != 0 && p.AddressBytes != 2 && p.AddressBytes != 3 && p.AddressBytes != 4 {
		return nil, fmt.Errorf("invalid PAC address width")
	}
	h = append(h, byte(p.AddressBytes))
	h = order.AppendUint32(h, p.RecordWidth)
	names := make([]string, 0, len(p.Headers))
	for name := range p.Headers {
		if name == "" || !utf8.ValidString(name) {
			return nil, fmt.Errorf("invalid PAC header name")
		}
		names = append(names, name)
	}
	sort.Strings(names)
	h = Extended(h, uint64(len(names)))
	for _, name := range names {
		h = Extended(h, uint64(len(name)))
		h = append(h, name...)
		h = Extended(h, uint64(len(p.Headers[name])))
		h = append(h, p.Headers[name]...)
	}
	h = Extended(h, uint64(len(p.Common)))
	h = append(h, p.Common...)
	var memberOffset uint64
	var directory []byte
	if p.AddressBytes > 0 {
		directory = make([]byte, len(p.Members)*p.AddressBytes)
	}
	for i, m := range p.Members {
		if len(m) == 0 || p.RecordWidth > 0 && len(m) != int(p.RecordWidth) {
			return nil, fmt.Errorf("invalid PAC member width")
		}
		if p.AddressBytes > 0 {
			if memberOffset >= uint64(1)<<(8*p.AddressBytes) {
				return nil, fmt.Errorf("PAC address overflow")
			}
			for j := 0; j < p.AddressBytes; j++ {
				directory[i*p.AddressBytes+j] = byte(memberOffset >> (8 * j))
			}
		}
		memberOffset += uint64(len(m))
	}
	if p.Directory != nil && !bytes.Equal(directory, p.Directory) {
		return nil, fmt.Errorf("PAC directory does not match final members")
	}
	p.Directory = directory
	h = append(h, directory...)
	order.PutUint64(h[16:24], uint64(len(h)))
	total := len(h)
	for _, m := range p.Members {
		if len(m) > PACMaximum-total {
			return nil, fmt.Errorf("PAC exceeds byte limit")
		}
		total += len(m)
	}
	order.PutUint32(h[:4], uint32(total))
	order.PutUint64(h[8:16], uint64(total))
	out := make([]byte, 0, total)
	out = append(out, h...)
	for _, m := range p.Members {
		out = append(out, m...)
	}
	return out, nil
}

// ParsePAC borrows all frame/extension bytes. supported lists understood required
// extension types; unknown optional types remain available but can be skipped.
func ParsePAC(l *Layout, raw []byte, supported map[uint32]bool) (*PAC, error) {
	if len(raw) < PACHeaderSize || uint64(order.Uint32(raw[:4])) != uint64(len(raw)) {
		return nil, fmt.Errorf("invalid PAC stored length")
	}
	flags := order.Uint32(raw[4:8])
	size := order.Uint64(raw[8:16])
	head := order.Uint64(raw[16:24])
	count := order.Uint64(raw[24:32])
	if flags & ^uint32(5) != 0 || flags&4 == 0 || size != uint64(len(raw)) || size > PACMaximum || head < PACHeaderSize || head > size {
		return nil, fmt.Errorf("invalid PAC bounds/features")
	}
	p := &PAC{Bytes: raw}
	copy(p.Digest[:], raw[32:64])
	if p.Digest != l.Digest {
		return nil, fmt.Errorf("PAC definition mismatch")
	}
	at := PACHeaderSize
	n, err := ReadExtended(raw[:head], &at)
	if err != nil {
		return nil, err
	}
	if n > uint64((int(head)-at)/10) {
		return nil, fmt.Errorf("invalid PAC extension count")
	}
	seen := map[uint32]bool{}
	for ; n > 0; n-- {
		if int(head)-at < 8 {
			return nil, io.ErrUnexpectedEOF
		}
		kind := order.Uint32(raw[at:])
		version := order.Uint16(raw[at+4:])
		ef := order.Uint16(raw[at+6:])
		at += 8
		length, e := ReadExtended(raw[:head], &at)
		if e != nil || length > uint64(int(head)-at) || seen[kind] || ef > 1 {
			return nil, fmt.Errorf("invalid PAC extension")
		}
		if ef == 1 && (version != 0 || !supported[kind]) {
			return nil, fmt.Errorf("unsupported required PAC extension %d", kind)
		}
		if supported[kind] && version != 0 {
			return nil, fmt.Errorf("unsupported PAC extension schema")
		}
		if kind == PACSource && version == 0 && !utf8.Valid(raw[at:at+int(length)]) {
			return nil, fmt.Errorf("invalid PAC source UTF-8")
		}
		seen[kind] = true
		p.Extensions = append(p.Extensions, PACExtension{kind, ef == 1, raw[at : at+int(length)]})
		at += int(length)
	}
	if int(head)-at < 5 {
		return nil, io.ErrUnexpectedEOF
	}
	p.AddressBytes = int(raw[at])
	p.RecordWidth = order.Uint32(raw[at+1:])
	at += 5
	if p.AddressBytes != 0 && p.AddressBytes != 2 && p.AddressBytes != 3 && p.AddressBytes != 4 {
		return nil, fmt.Errorf("invalid PAC address width")
	}
	names, e := ReadExtended(raw[:head], &at)
	if e != nil || names > uint64((int(head)-at)/4) {
		return nil, fmt.Errorf("invalid PAC headers")
	}
	p.Headers = map[string][]byte{}
	lastName := ""
	for ; names > 0; names-- {
		n, e := ReadExtended(raw[:head], &at)
		if e != nil || n == 0 || n > head-uint64(at) {
			return nil, fmt.Errorf("invalid PAC header name")
		}
		name := string(raw[at : at+int(n)])
		at += int(n)
		if !utf8.ValidString(name) || name <= lastName {
			return nil, fmt.Errorf("invalid PAC header order")
		}
		lastName = name
		n, e = ReadExtended(raw[:head], &at)
		if e != nil || n > head-uint64(at) {
			return nil, fmt.Errorf("invalid PAC header value")
		}
		p.Headers[name] = raw[at : at+int(n)]
		at += int(n)
	}
	common, e := ReadExtended(raw[:head], &at)
	if e != nil || common > head-uint64(at) || (common > 0) != (flags&PACCommon != 0) {
		return nil, fmt.Errorf("invalid PAC common frame")
	}
	p.Common = raw[at : at+int(common)]
	at += int(common)
	if count*uint64(p.AddressBytes) != head-uint64(at) {
		return nil, fmt.Errorf("invalid PAC directory length")
	}
	p.Directory = raw[at:head]
	var prefix Frame
	if common > 0 {
		if e = l.scanFragment(p.Common, &prefix); e != nil {
			return nil, e
		}
	}
	mask := (len(l.Widths) + 3) / 4
	if mask == 0 || count > uint64((len(raw)-int(head))/mask) || count > math.MaxInt || count > PACMaximumMembers {
		return nil, fmt.Errorf("invalid PAC member count")
	}
	p.Members = make([][]byte, 0, int(count))
	r := bytes.NewReader(raw[head:])
	var scan Frame
	var scratch []byte
	for i := uint64(0); i < count; i++ {
		start := len(raw) - r.Len()
		if scratch, e = l.ReadFrame(r, scratch); e != nil {
			return nil, e
		}
		member := raw[start : len(raw)-r.Len()]
		if e = l.scanFragment(member, &scan); e != nil {
			return nil, e
		}
		for f, v := range prefix.Values {
			if v.State != Absent && scan.Values[f].State != Absent {
				return nil, fmt.Errorf("PAC common field overridden")
			}
		}
		if l.schema != nil {
			if _, err := ResolvePAC(l, p.Common, member); err != nil {
				return nil, err
			}
		}
		ordinal := int(i)
		if p.RecordWidth > 0 && len(member) != int(p.RecordWidth) {
			return nil, fmt.Errorf("PAC fixed member width mismatch")
		}
		if p.AddressBytes > 0 {
			var offset uint64
			for j := 0; j < p.AddressBytes; j++ {
				offset |= uint64(p.Directory[ordinal*p.AddressBytes+j]) << (8 * j)
			}
			if offset != uint64(start)-head {
				return nil, fmt.Errorf("PAC member directory mismatch")
			}
		}
		p.Members = append(p.Members, member)
	}
	if r.Len() != 0 {
		return nil, fmt.Errorf("PAC trailing bytes")
	}
	return p, nil
}

// ResolvePAC is an explicit standalone export, not the presentation/storage path.
func ResolvePAC(l *Layout, common, member []byte) ([]byte, error) {
	var c, m Frame
	if err := l.scanFragment(member, &m); err != nil {
		return nil, err
	}
	if len(common) > 0 {
		if err := l.scanFragment(common, &c); err != nil {
			return nil, err
		}
		for i, v := range c.Values {
			if v.State != Absent {
				if m.Values[i].State != Absent {
					return nil, fmt.Errorf("PAC override")
				}
				m.Values[i] = v
			}
		}
	}
	return l.Encode(nil, m.Values)
}
