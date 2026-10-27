package codec

// Per-event compression is a negotiated envelope, not a new PIL primitive.
// The reserved NULL event-identity mask cannot be a valid input event or a
// stripped population (whose event identity is ABSENT).
import (
	"bytes"
	"compress/gzip"
	"fmt"
	"io"
	"sync"
)

const CompressionFeature = "event-gzip-v1"
const compressionHeader = 16 // PC, version, codec; identity/body/stored lengths (LE u32)
var gzipWriters = sync.Pool{New: func() any { w, _ := gzip.NewWriterLevel(io.Discard, 6); return w }}
var fastGzipWriters = sync.Pool{New: func() any { w, _ := gzip.NewWriterLevel(io.Discard, 1); return w }}
var gzipReaders sync.Pool

func (l *Layout) compressedMarker(raw []byte) bool {
	m := (len(l.Widths) + 3) / 4
	if l.Identity.Primary < 0 || l.Identity.Primary >= len(l.Widths) || len(raw) < m {
		return false
	}
	for i, b := range raw[:m] {
		var want byte
		if i == l.Identity.Primary/4 {
			want = 1 << uint(6-2*(l.Identity.Primary%4))
		}
		if b != want {
			return false
		}
	}
	return true
}
func (l *Layout) IsCompressed(raw []byte) bool { return l.compressedMarker(raw) }
func (l *Layout) CompressionLengths(raw []byte) (head, identity, plain, stored int, err error) {
	head = (len(l.Widths)+3)/4 + compressionHeader
	if !l.compressedMarker(raw) || len(raw) < head {
		err = fmt.Errorf("short compressed event")
		return
	}
	h := raw[head-compressionHeader : head]
	if !bytes.Equal(h[:4], []byte{'P', 'C', 1, 1}) {
		err = fmt.Errorf("unsupported event compression")
		return
	}
	identity = int(order.Uint32(h[4:8]))
	plain = int(order.Uint32(h[8:12]))
	stored = int(order.Uint32(h[12:16]))
	if identity <= 0 || plain <= 0 || stored <= 0 || identity > MaxFrame || plain > MaxFrame-identity || stored > MaxFrame-head-identity {
		err = fmt.Errorf("invalid compressed event lengths")
	}
	return
}
func (l *Layout) IdentityField(i int) bool { return i == l.Identity.Primary || l.Identity.Fields[i] }

// CompressEvent returns the original frame when the complete envelope would
// not save bytes. Only positional values are used; no semantic object map.
func (l *Layout) CompressEvent(raw []byte) ([]byte, error) { return l.CompressEventLevel(raw, 1) }
func (l *Layout) CompressEventLevel(raw []byte, level int) ([]byte, error) {
	if l.Identity.Primary < 0 {
		return nil, fmt.Errorf("compression requires an identity profile")
	}
	pool := &gzipWriters
	if level == 1 {
		pool = &fastGzipWriters
	} else if level != 6 {
		return nil, fmt.Errorf("event compression level must be 1 or 6")
	}
	if l.IsCompressed(raw) {
		return nil, fmt.Errorf("nested compression")
	}
	var f Frame
	if err := l.Scan(raw, &f); err != nil {
		return nil, err
	}
	if err := l.validateIdentity(f.Values[l.Identity.Primary]); err != nil {
		return nil, err
	}
	// Skip the compression attempt for tiny revisions: CPU savings take priority.
	if len(raw) < 80 {
		return raw, nil
	}
	ids := make([]Value, len(f.Values))
	for i := range ids {
		if l.IdentityField(i) {
			ids[i] = f.Values[i]
			f.Values[i] = Value{}
		}
	}
	identity, err := l.Encode(nil, ids)
	if err != nil {
		return nil, err
	}
	body, err := l.Encode(nil, f.Values)
	if err != nil {
		return nil, err
	}
	var buffer bytes.Buffer
	w := pool.Get().(*gzip.Writer)
	w.Reset(&buffer)
	_, err = w.Write(body)
	if err == nil {
		err = w.Close()
	}
	w.Reset(io.Discard)
	pool.Put(w)
	if err != nil {
		return nil, err
	}
	m := (len(l.Widths) + 3) / 4
	if m+compressionHeader+len(identity)+buffer.Len() >= len(raw) {
		return raw, nil
	}
	out := make([]byte, m, m+compressionHeader+len(identity)+buffer.Len())
	out[l.Identity.Primary/4] = 1 << uint(6-2*(l.Identity.Primary%4))
	out = append(out, 'P', 'C', 1, 1)
	out = order.AppendUint32(out, uint32(len(identity)))
	out = order.AppendUint32(out, uint32(len(body)))
	out = order.AppendUint32(out, uint32(buffer.Len()))
	out = append(out, identity...)
	out = append(out, buffer.Bytes()...)
	return out, nil
}

func (l *Layout) scanCompressed(raw []byte, reuse *Frame) error {
	head, n, plain, stored, err := l.CompressionLengths(raw)
	if err != nil {
		return err
	}
	if len(raw) != head+n+stored {
		return fmt.Errorf("compressed event boundary mismatch")
	}
	if reuse.compressedIdentity == nil {
		reuse.compressedIdentity = &Frame{}
		reuse.compressedBody = &Frame{}
	}
	identity := reuse.compressedIdentity
	body := reuse.compressedBody
	if err = l.ScanPlain(raw[head:head+n], identity); err != nil {
		return err
	}
	if err = l.validateIdentity(identity.Values[l.Identity.Primary]); err != nil {
		return err
	}
	input := bytes.NewReader(raw[head+n:])
	var reader *gzip.Reader
	if pooled := gzipReaders.Get(); pooled != nil {
		reader = pooled.(*gzip.Reader)
		err = reader.Reset(input)
	} else {
		reader, err = gzip.NewReader(input)
	}
	if err != nil {
		return err
	}
	reader.Multistream(false)
	if cap(reuse.inflated) < plain+1 {
		reuse.inflated = make([]byte, plain+1)
	} else {
		reuse.inflated = reuse.inflated[:plain+1]
	}
	count, readErr := io.ReadFull(reader, reuse.inflated)
	reader.Close()
	remaining := input.Len()
	_ = reader.Reset(bytes.NewReader(nil)) // release the borrowed network/storage buffer
	gzipReaders.Put(reader)
	if readErr != io.ErrUnexpectedEOF || count != plain || remaining != 0 {
		return fmt.Errorf("invalid gzip event length or checksum: %v", readErr)
	}
	if err = l.ScanPlain(reuse.inflated[:plain], body); err != nil {
		return err
	}
	if cap(reuse.Values) < len(l.Widths) {
		reuse.Values = make([]Value, len(l.Widths))
	} else {
		reuse.Values = reuse.Values[:len(l.Widths)]
	}
	for i := range reuse.Values {
		if l.IdentityField(i) {
			if body.Values[i].State != Absent {
				return fmt.Errorf("compressed body duplicates identity")
			}
			reuse.Values[i] = identity.Values[i]
		} else {
			if identity.Values[i].State != Absent {
				return fmt.Errorf("non-identity in compression header")
			}
			reuse.Values[i] = body.Values[i]
		}
	}
	reuse.Bytes = raw
	return nil
}

// ExpandEvent is the compatibility path for peers without envelope support.
// Negotiated peers receive the original envelope, without recompression.
func (l *Layout) ExpandEvent(raw []byte) ([]byte, error) {
	if !l.IsCompressed(raw) {
		return raw, nil
	}
	var frame Frame
	if err := l.Scan(raw, &frame); err != nil {
		return nil, err
	}
	return l.Encode(nil, frame.Values)
}

// IdentityFrame reads only the clear prefix of an already validated event.
// Consumers can inspect identity fields without inflating the body.
func (l *Layout) IdentityFrame(raw []byte, reuse *Frame) error {
	if !l.IsCompressed(raw) {
		return l.Scan(raw, reuse)
	}
	head, n, _, stored, err := l.CompressionLengths(raw)
	if err != nil {
		return err
	}
	if len(raw) != head+n+stored {
		return fmt.Errorf("compressed identity boundary mismatch")
	}
	return l.ScanPlain(raw[head:head+n], reuse)
}

func (l *Layout) validateIdentity(v Value) error {
	if l.Identity.Validate != nil {
		return l.Identity.Validate(v)
	}
	if v.State != Direct || len(v.Bytes) == 0 {
		return fmt.Errorf("missing compression identity")
	}
	return nil
}
