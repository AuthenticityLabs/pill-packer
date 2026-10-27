package codec

import (
	"bytes"
	"compress/gzip"
	"fmt"
	"io"
)

// Gzip compresses a byte stream; framing and compression policy belong to the caller.
func Gzip(dst io.Writer, src io.Reader, level int) error {
	z, err := gzip.NewWriterLevel(dst, level)
	if err != nil {
		return err
	}
	if _, err = io.Copy(z, src); err != nil {
		_ = z.Close()
		return err
	}
	return z.Close()
}

// GzipReader validates trailers as the caller reads through EOF. It accepts
// concatenated gzip members, as required by gzip stream transport.
func GzipReader(src io.Reader) (io.ReadCloser, error) { return gzip.NewReader(src) }

// InflateGzip bounds decoded memory and rejects extra members or trailing bytes.
func InflateGzip(raw []byte, expected int, limit int) ([]byte, error) {
	if expected < 0 || expected > limit {
		return nil, fmt.Errorf("decoded length exceeds limit")
	}
	input := bytes.NewReader(raw)
	z, err := gzip.NewReader(input)
	if err != nil {
		return nil, err
	}
	z.Multistream(false)
	out, err := io.ReadAll(io.LimitReader(z, int64(expected)+1))
	closeErr := z.Close()
	if err != nil {
		return nil, err
	}
	if closeErr != nil {
		return nil, closeErr
	}
	if len(out) != expected || input.Len() != 0 {
		return nil, fmt.Errorf("invalid decoded length or trailing compressed bytes")
	}
	return out, nil
}
