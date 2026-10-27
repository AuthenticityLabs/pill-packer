package codec

import (
	"bytes"
	"compress/gzip"
	"fmt"
	"io"
)

// The compressed representation has only framing/decoding controls outside gzip.
// Its body expands to the original PAC header, extensions, prefix, and records.
func CompressPAC(raw []byte, level int) ([]byte, error) {
	if err := ValidatePlainPACEnvelope(raw); err != nil {
		return nil, err
	}
	var body bytes.Buffer
	z, err := gzip.NewWriterLevel(&body, level)
	if err != nil {
		return nil, err
	}
	if _, err = z.Write(raw); err != nil {
		return nil, err
	}
	if err = z.Close(); err != nil {
		return nil, err
	}
	if body.Len()+PACCompressedHeaderSize >= len(raw) {
		return raw, nil
	}
	out := make([]byte, PACCompressedHeaderSize, PACCompressedHeaderSize+body.Len())
	order.PutUint32(out[0:4], uint32(PACCompressedHeaderSize+body.Len()))
	order.PutUint32(out[4:8], PACGzip)
	order.PutUint32(out[8:12], uint32(len(raw)))
	order.PutUint32(out[12:16], uint32(order.Uint64(raw[24:32])))
	return append(out, body.Bytes()...), nil
}

func ValidatePlainPACEnvelope(raw []byte) error {
	if len(raw) < PACHeaderSize || uint64(len(raw)) > PACMaximum ||
		uint64(order.Uint32(raw[:4])) != uint64(len(raw)) || order.Uint32(raw[4:8]) > PACCommon ||
		order.Uint64(raw[8:16]) != uint64(len(raw)) || order.Uint64(raw[16:24]) < PACHeaderSize ||
		order.Uint64(raw[16:24]) > uint64(len(raw)) || order.Uint64(raw[24:32]) > PACMaximumMembers {
		return fmt.Errorf("invalid decoded PAC envelope")
	}
	return nil
}

func ExpandPAC(raw []byte) ([]byte, error) {
	if len(raw) < 8 || uint64(order.Uint32(raw[:4])) != uint64(len(raw)) {
		return nil, fmt.Errorf("invalid PAC stored length")
	}
	flags := order.Uint32(raw[4:8])
	if flags <= PACCommon {
		if err := ValidatePlainPACEnvelope(raw); err != nil {
			return nil, err
		}
		return raw, nil
	}
	if flags != PACGzip || len(raw) <= PACCompressedHeaderSize {
		return nil, fmt.Errorf("invalid PAC compression controls")
	}
	decoded, count := order.Uint32(raw[8:12]), order.Uint32(raw[12:16])
	if decoded < PACHeaderSize || uint64(count) > uint64(decoded)-PACHeaderSize {
		return nil, fmt.Errorf("invalid PAC compression bounds")
	}
	input := bytes.NewReader(raw[PACCompressedHeaderSize:])
	z, err := gzip.NewReader(input)
	if err != nil {
		return nil, err
	}
	z.Multistream(false)
	out, err := io.ReadAll(io.LimitReader(z, int64(decoded)+1))
	closeErr := z.Close()
	if err != nil {
		return nil, err
	}
	if closeErr != nil {
		return nil, closeErr
	}
	if uint64(len(out)) != uint64(decoded) || input.Len() != 0 {
		return nil, fmt.Errorf("invalid decoded PAC length or trailing compressed data")
	}
	if err := ValidatePlainPACEnvelope(out); err != nil {
		return nil, err
	}
	if order.Uint64(out[24:32]) != uint64(count) {
		return nil, fmt.Errorf("PAC compressed/decoded count mismatch")
	}
	return out, nil
}
