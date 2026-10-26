package pillpacker

import (
	"bytes"
	"errors"
	"sync"
	"testing"
)

func TestRustDefinitionAndBorrowedFrames(t *testing.T) {
	d, err := FromJSON([]byte(`{"pil":0,"definition":"demo","version":0,"fields":[{"name":"n","type":"U16"}],"vendor":{"purpose":"test"}}`))
	if err != nil {
		t.Fatal(err)
	}
	defer d.Close()
	encoded, err := d.CBOR()
	if err != nil {
		t.Fatal(err)
	}
	next, err := FromCBOR(encoded)
	if err != nil {
		t.Fatal(err)
	}
	defer next.Close()
	forwarded, _ := next.CBOR()
	if !bytes.Equal(encoded, forwarded) || !bytes.Contains(encoded, []byte("vendor")) {
		t.Fatal("lost annotation")
	}
	frame := []byte{0x80, 42, 0}
	var readers sync.WaitGroup
	for i := 0; i < 8; i++ {
		readers.Add(1)
		go func() {
			defer readers.Done()
			for j := 0; j < 100; j++ {
				n, err := d.Scan(frame, 1024)
				if err != nil || n != 3 {
					t.Errorf("scan: %d %v", n, err)
					return
				}
			}
		}()
	}
	readers.Wait()
	if _, err := d.Scan(frame[:2], 1024); !errors.Is(err, ErrIncomplete) {
		t.Fatal(err)
	}
	if allocations := testing.AllocsPerRun(100, func() { _, _ = d.Scan(frame, 1024) }); allocations != 0 {
		t.Fatalf("hot scan: %g Go allocations", allocations)
	}
	d.Close()
	d.Close()
	if _, err := d.Scan(frame, 1024); !errors.Is(err, ErrClosed) {
		t.Fatal(err)
	}
}

func TestInvalidDefinition(t *testing.T) {
	if d, err := FromJSON([]byte(`{"pil":0}`)); err == nil {
		d.Close()
		t.Fatal("accepted missing fields")
	}
}
