// Package pillpacker wraps the Rust codec. Definitions and scan scratch space
// live in Rust; record input is borrowed synchronously without a second copy.
package pillpacker

/*
#cgo CFLAGS: -I${SRCDIR}/../include
#cgo LDFLAGS: ${SRCDIR}/../rust/ffi/target/release/libpill_packer_ffi.a
#cgo darwin LDFLAGS: -framework Security -framework CoreFoundation -liconv -lm
#cgo linux LDFLAGS: -ldl -lpthread -lm -lrt -lutil
#include "pill_packer.h"
*/
import "C"
import (
	"errors"
	"runtime"
	"sync"
	"unsafe"
)

var ErrIncomplete = errors.New("incomplete PIL data")
var ErrClosed = errors.New("PIL definition is closed")

type Definition struct {
	mu  sync.Mutex
	ctx *C.pp_context
}

func pointer(b []byte) *C.uint8_t { return (*C.uint8_t)(unsafe.Pointer(unsafe.SliceData(b))) }

func from(bytes []byte, format C.uint8_t) (*Definition, error) {
	d := &Definition{ctx: C.pp_new()}
	if d.ctx == nil {
		return nil, errors.New("could not allocate PIL context")
	}
	result := C.pp_define(d.ctx, pointer(bytes), C.size_t(len(bytes)), format)
	runtime.KeepAlive(bytes)
	if result < 0 {
		err := d.failure(result)
		d.Close()
		return nil, err
	}
	runtime.SetFinalizer(d, (*Definition).Close)
	return d, nil
}
func FromCBOR(bytes []byte) (*Definition, error) { return from(bytes, 0) }
func FromJSON(bytes []byte) (*Definition, error) { return from(bytes, 1) }

func (d *Definition) failure(result C.int64_t) error {
	if result == -2 {
		return ErrIncomplete
	}
	var size C.size_t
	ptr := C.pp_error(d.ctx, &size)
	return errors.New(string(unsafe.Slice((*byte)(unsafe.Pointer(ptr)), int(size))))
}

// CBOR copies the complete document, preserving opaque annotations and exact
// original bytes for a CBOR import. Call once during registration/forwarding.
func (d *Definition) CBOR() ([]byte, error) {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.ctx == nil {
		return nil, ErrClosed
	}
	var size C.size_t
	ptr := C.pp_document(d.ctx, &size)
	out := append([]byte(nil), unsafe.Slice((*byte)(unsafe.Pointer(ptr)), int(size))...)
	runtime.KeepAlive(d)
	return out, nil
}

// Scan returns the first frame's byte length. It validates frame structure;
// profile/business validation is separate. maximum bounds the decoded frame.
func (d *Definition) Scan(bytes []byte, maximum int) (int, error) {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.ctx == nil {
		return 0, ErrClosed
	}
	if maximum <= 0 {
		return 0, errors.New("maximum must be positive")
	}
	result := C.pp_scan(d.ctx, pointer(bytes), C.size_t(len(bytes)), C.size_t(maximum))
	runtime.KeepAlive(bytes)
	runtime.KeepAlive(d)
	if result < 0 {
		return 0, d.failure(result)
	}
	return int(result), nil
}

func (d *Definition) Close() {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.ctx != nil {
		C.pp_free(d.ctx)
		d.ctx = nil
	}
	runtime.SetFinalizer(d, nil)
}
