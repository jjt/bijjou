// Package stream holds the incremental path: bijjou reads stdin in batches
// and writes each batch out before it reads the next one.
package stream

import (
	"bufio"
	"bytes"
	"errors"
	"io"
	"os"
	"strconv"
	"strings"
	"syscall"

	"github.com/charmbracelet/x/term"

	"tangled.org/jjt.io/bijjou/internal/ansi"
	"tangled.org/jjt.io/bijjou/internal/config"
	"tangled.org/jjt.io/bijjou/internal/hydra"
	"tangled.org/jjt.io/bijjou/internal/output"
	"tangled.org/jjt.io/bijjou/internal/pipeline"
	"tangled.org/jjt.io/bijjou/internal/render"
)

// Run drives the streaming path end to end.
func Run() error {
	c := config.Get()
	firstSize, restSize := resolveBatchSizes(c.StreamBatchSize)
	sink := output.Open()
	// The bookmark naming comes from hydra.prefixes. No lookup can overlap.
	hy := hydra.NewWalk()
	reader := bufio.NewReader(os.Stdin)
	templates, err := pipeline.CompileTemplates(c.Templates)
	if err != nil {
		return err
	}
	metrics := make(map[string]*pipeline.Metrics)
	maxGraphCol := 0

	first, err := readBatch(reader, firstSize)
	if err != nil {
		return err
	}
	if len(first) == 0 {
		return sink.Close()
	}

	if c.Activate == config.ModeAuto {
		concatenatedLen := 0
		for _, line := range first {
			concatenatedLen += len(line)
		}
		joined := make([]byte, 0, concatenatedLen)
		for _, line := range first {
			joined = append(joined, line...)
		}
		if !bytes.Contains(joined, []byte(config.BijjouTemplateNameField)) {
			if err := sinkWrite(sink, joined); err != nil {
				return err
			}
			if err := passthrough(reader, sink); err != nil {
				return err
			}
			return sink.Close()
		}
	}

	var out bytes.Buffer
	if err := processBatch(first, templates, metrics, &maxGraphCol, hy, sink, &out); err != nil {
		return err
	}
	for {
		batch, err := readBatch(reader, restSize)
		if err != nil {
			return err
		}
		if len(batch) == 0 {
			break
		}
		if err := processBatch(batch, templates, metrics, &maxGraphCol, hy, sink, &out); err != nil {
			return err
		}
	}
	return sink.Close()
}

// resolveBatchSizes turns the configured batch size into the first-batch and
// the follow-up batch line counts.
func resolveBatchSizes(bs config.BatchSize) (int, int) {
	if !bs.HalfPager {
		n := bs.Fixed
		if n < 1 {
			n = 1
		}
		return n, n
	}
	h := 24
	if config.Get().DebugForceScreenHeight != 0 {
		h = config.Get().DebugForceScreenHeight
	} else if th, ok := terminalHeight(); ok {
		h = th
	}
	first := h - 1
	if first < 1 {
		first = 1
	}
	rest := first / 2
	if rest < 1 {
		rest = 1
	}
	return first, rest
}

// terminalHeight asks the standard streams for their window size, then
// /dev/tty, then the LINES environment variable.
func terminalHeight() (int, bool) {
	for _, f := range []*os.File{os.Stderr, os.Stdout, os.Stdin} {
		if _, h, err := term.GetSize(f.Fd()); err == nil && h > 0 {
			return h, true
		}
	}
	if f, err := os.OpenFile("/dev/tty", os.O_RDWR, 0); err == nil {
		_, h, err := term.GetSize(f.Fd())
		_ = f.Close()
		if err == nil && h > 0 {
			return h, true
		}
	}
	if s, ok := os.LookupEnv("LINES"); ok {
		if n, err := strconv.ParseUint(strings.TrimPrefix(s, "+"), 10, 32); err == nil {
			return int(n), true
		}
	}
	return 0, false
}

// readBatch reads up to batchSize lines. Every line keeps its newline; a
// final line without one is kept as is.
func readBatch(reader *bufio.Reader, batchSize int) ([][]byte, error) {
	out := make([][]byte, 0, batchSize)
	for i := 0; i < batchSize; i++ {
		buf, err := reader.ReadBytes('\n')
		if len(buf) > 0 {
			out = append(out, buf)
		}
		if err != nil {
			if errors.Is(err, io.EOF) {
				break
			}
			return nil, err
		}
	}
	return out, nil
}

// processBatch classifies, measures and emits one batch of lines.
func processBatch(batch [][]byte, templates map[string]*pipeline.CompiledTemplate, metrics map[string]*pipeline.Metrics, maxGraphCol *int, hy *hydra.Walk, sink *output.Sink, out *bytes.Buffer) error {
	rows := make([]pipeline.Row, len(batch))
	size := 0
	for i, l := range batch {
		body, _ := render.StripTrailingNL(l)
		rows[i] = pipeline.ClassifyRow(body)
		size += len(l) + 16
	}

	// Monotonic widen: anchors only grow across batches. As a result, the
	// rows already emitted above stay valid because column targets never
	// shrink.
	pipeline.AccumulateMetrics(rows, templates, metrics, maxGraphCol)

	out.Reset()
	out.Grow(size)
	for i, line := range batch {
		pipeline.EmitClassified(line, &rows[i], templates, metrics, *maxGraphCol, hy, out)
	}
	if config.ColorEnabled() {
		return sinkWrite(sink, out.Bytes())
	}
	return sinkWrite(sink, ansi.StripSGR(out.Bytes()))
}

// passthrough copies the rest of the reader to the sink unchanged.
func passthrough(reader io.Reader, sink *output.Sink) error {
	buf := make([]byte, 8192)
	for {
		n, err := reader.Read(buf)
		if n > 0 {
			if werr := sinkWrite(sink, buf[:n]); werr != nil {
				return werr
			}
		}
		if err != nil {
			if errors.Is(err, io.EOF) {
				return nil
			}
			if errors.Is(err, syscall.EINTR) {
				continue
			}
			return err
		}
	}
}

// sinkWrite writes to the sink. A broken pipe means the pager stopped
// reading, so bijjou marks the sink closed and returns without an error.
func sinkWrite(sink *output.Sink, buf []byte) error {
	err := sink.Write(buf)
	if err == nil {
		return nil
	}
	if errors.Is(err, syscall.EPIPE) {
		sink.MarkClosed()
		return nil
	}
	return err
}
