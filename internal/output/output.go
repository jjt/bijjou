// Package output owns the destination bijjou writes to: stdout or a spawned
// pager child.
package output

import (
	"io"
	"os"
	"os/exec"
	"strings"

	"github.com/charmbracelet/x/term"

	"tangled.org/jjt.io/bijjou/internal/ansi"
	"tangled.org/jjt.io/bijjou/internal/config"
)

// WriteOutput sends one finished buffer to the sink, stripping colour first
// when colour is off.
func WriteOutput(buf []byte) error {
	filtered := buf
	if !config.ColorEnabled() {
		filtered = ansi.StripSGR(buf)
	}
	sink := Open()
	if err := sink.Write(filtered); err != nil {
		return err
	}
	return sink.Close()
}

// Sink is an output destination: either stdout or a spawned pager child. The
// buffered path (WriteOutput) and the streaming path share it.
type Sink struct {
	// cmd is nil for the stdout sink.
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	closed bool
}

// Open spawns a pager when bijjou is configured for one and PAGER holds a
// usable value. If not, it writes to stdout. The auto mode pages only on a
// TTY. The always mode pages when PAGER is set. The never mode does not page.
// A failed spawn falls back to stdout.
func Open() *Sink {
	isTTY := term.IsTerminal(os.Stdout.Fd())
	pagerVar := os.Getenv("PAGER")
	hasPager := strings.TrimSpace(pagerVar) != ""
	wantPage := false
	switch config.Get().Pager {
	case config.ModeNever:
		wantPage = false
	case config.ModeAlways:
		wantPage = hasPager
	case config.ModeAuto:
		wantPage = isTTY && hasPager
	}
	if wantPage {
		if sink := spawnPager(pagerVar); sink != nil {
			return sink
		}
	}
	return &Sink{}
}

// MarkClosed records that the child stopped reading. Later writes are drops.
func (s *Sink) MarkClosed() {
	if s.cmd != nil {
		s.closed = true
	}
}

// Write sends buf to the sink.
func (s *Sink) Write(p []byte) error {
	if s.cmd == nil {
		_, err := os.Stdout.Write(p)
		return err
	}
	if s.closed {
		return nil
	}
	_, err := s.stdin.Write(p)
	return err
}

// Close flushes stdout, or closes the child stdin and reaps the pager.
func (s *Sink) Close() error {
	if s.cmd == nil {
		return nil
	}
	if s.stdin != nil {
		_ = s.stdin.Close()
		s.stdin = nil
	}
	_ = s.cmd.Wait()
	return nil
}

// spawnPager starts the pager named by cmdLine with a piped stdin. It returns
// nil when the command line is empty or the spawn fails.
func spawnPager(cmdLine string) *Sink {
	parts := strings.Fields(cmdLine)
	if len(parts) == 0 {
		return nil
	}
	cmd := exec.Command(parts[0], parts[1:]...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil
	}
	if err := cmd.Start(); err != nil {
		_ = stdin.Close()
		return nil
	}
	return &Sink{cmd: cmd, stdin: stdin}
}
