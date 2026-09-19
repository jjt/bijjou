// Package cli builds the bijjou command line and runs the post-processor.
package cli

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/signal"
	"runtime/debug"
	"strings"
	"syscall"

	"charm.land/log/v2"
	"github.com/charmbracelet/fang"
	"github.com/spf13/cobra"

	"tangled.org/jjt.io/bijjou/internal/config"
	"tangled.org/jjt.io/bijjou/internal/pipeline"
	"tangled.org/jjt.io/bijjou/internal/stream"
)

// longHelp is the reference bijjou prints for `--help`. The usage and flag
// sections come from the command itself, so this text starts at the
// configuration reference. README.md points the reader here.
const longHelp = "bijjou - jj log post-processor\n" +
	"\n" +
	"CONFIGURATION\n" +
	"  Precedence (low to high): config file < env vars < CLI flags.\n" +
	"\n" +
	"  Config file paths (first match wins):\n" +
	"    $BIJJOU_CONFIG\n" +
	"    $XDG_CONFIG_HOME/bijjou/config.toml\n" +
	"    $HOME/.config/bijjou/config.toml\n" +
	"\n" +
	"  Env vars: prefix BIJJOU__, replace '.' with '__' and '-' with '_'.\n" +
	"    Uppercase is canonical. Lowercase is also accepted. For example:\n" +
	"      graph.edges.chars.horizontal -> BIJJOU__GRAPH__EDGES__CHARS__HORIZONTAL=X\n" +
	"      layout.dash-start            -> BIJJOU__LAYOUT__DASH_START=<\n" +
	"      activate                     -> BIJJOU__ACTIVATE=auto\n" +
	"\n" +
	"  CLI flags: --<key>=<value>, replace '.' with '__' (hyphens are kept as-is).\n" +
	"    For example:\n" +
	"      graph.edges.chars.horizontal -> --graph__edges__chars__horizontal=X\n" +
	"      layout.dash-start            -> --layout__dash-start=<\n" +
	"      templates.log_oneline        -> --templates__log_oneline='...'\n" +
	"\n" +
	"  Streaming mode flushes output in batches as input arrives. bijjou pre-scans\n" +
	"  the first batch, so every line in it shares the batch-wide max graph_col.\n" +
	"  Later batches widen monotonically per line as wider rows arrive. Alignment\n" +
	"  never shifts backwards. In streaming `auto` activation mode, bijjou limits\n" +
	"  the scan for the `bijjou_template_name` field to the first batch. If the\n" +
	"  field is not there, the rest of stdin passes through verbatim.\n" +
	"\n" +
	"KEYS\n" +
	"  activate                                  auto|always|never\n" +
	"  pager                                     auto|always|never\n" +
	"\n" +
	"  [ui]\n" +
	"    color                                   auto|always|never\n" +
	"\n" +
	"  [layout]\n" +
	"    dash                                    string\n" +
	"    dash-start                              string\n" +
	"    dash-end                                string (default empty: the\n" +
	"                                            closing cell is a space, so\n" +
	"                                            content keeps a space at its left)\n" +
	"\n" +
	"  [templates]\n" +
	"    <name>                                  DSL string (see bijjou-config.toml).\n" +
	"                                            Each row's `bijjou_template_name`\n" +
	"                                            field selects `templates.<name>`.\n" +
	"\n" +
	"  [stream]\n" +
	"    enabled                                 bool (default true)\n" +
	"    batch-size                              int >= 1 (default 128)\n" +
	"\n" +
	"  [graph]\n" +
	"    collapse                                bool (default false); drop jj's\n" +
	"                                            inter-column pad cells so the\n" +
	"                                            graph is half as wide\n" +
	"\n" +
	"  [graph.edges.chars]                       string (each)\n" +
	"    horizontal  vertical\n" +
	"    top-left  top-right  bottom-left  bottom-right\n" +
	"    tee-right  tee-left  tee-down  tee-up\n" +
	"    cross  elision\n" +
	"\n" +
	"  [colors]                                  int 0-255 | \"#rrggbb\"\n" +
	"    dash-filler  graph-edge\n" +
	"\n" +
	"  [hydra]\n" +
	"    enable                                  bool (default true); mark up the\n" +
	"                                            rows whose bookmarks match\n" +
	"                                            [hydra.prefixes]\n" +
	"    top-stack-padding                       bool (default true); draw the\n" +
	"                                            separator row jj skips under the\n" +
	"                                            log's top stack\n" +
	"    colors                                  true|false|list (default true);\n" +
	"                                            color each stack's graph nodes,\n" +
	"                                            and its HYWC-* working copy row.\n" +
	"                                            true hashes the stack name; a\n" +
	"                                            list of `int 0-255 | \"#rrggbb\"`\n" +
	"                                            is indexed by the order the log\n" +
	"                                            first names each stack\n" +
	"    color-bookmarks                         bool (default true); print the\n" +
	"                                            HYS-* and HYWC-* bookmark names\n" +
	"                                            in their stack's color too\n" +
	"\n" +
	"  [hydra.prefixes]                          string (each); how this repo\n" +
	"                                            names its hydra bookmarks\n" +
	"    prefix                                  default \"HY\"\n" +
	"    base  head  conflict-resolution         defaults \"B\", \"H\", \"CR\";\n" +
	"                                            whole names, e.g. `HYCR`\n" +
	"    stack-head  stack-working-copy          defaults \"S\", \"WC\"; followed\n" +
	"                                            by `-<stack>`, e.g. `HYWC-foo`\n" +
	"\n" +
	"  [hydra.prefixes-replace]                  string (each); what those names\n" +
	"                                            read as; same keys, each unset\n" +
	"                                            by default, classification\n" +
	"                                            unchanged\n" +
	"    prefix                                  stands in for the `HY` leader\n" +
	"                                            alone, e.g. `⋔S-foo`\n" +
	"    base  head  conflict-resolution         stand in for the whole name,\n" +
	"                                            e.g. base = \"◆\" for `HYB`\n" +
	"    stack-head  stack-working-copy          stand in for the leader and its\n" +
	"                                            dash, e.g. `Ψfoo` for `HYS-foo`\n" +
	"\n" +
	"See bijjou-config.toml for defaults and discussion."

// diag writes the diagnostics to stderr. The messages are a contract with the
// user and with the tests, so the logger reports no timestamp, no level and no
// prefix; it prints the message and nothing else.
var diag = log.New(os.Stderr)

// Execute runs bijjou and reports the process exit code: 0 on success, 1 on a
// runtime error and 2 on a configuration or usage error.
func Execute(ctx context.Context) int {
	code := 0
	// A reader that leaves early must end bijjou quietly. Go kills the process
	// on SIGPIPE unless the signal has a handler, so catch it and let the write
	// report the broken pipe as an error.
	signal.Notify(make(chan os.Signal, 1), syscall.SIGPIPE)
	root := newRootCmd(&code)
	if err := fang.Execute(
		ctx,
		root,
		fang.WithVersion(version()),
		fang.WithNotifySignal(os.Interrupt),
	); err != nil {
		if code == 0 {
			code = 1
		}
	}
	return code
}

func newRootCmd(code *int) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "bijjou",
		Short: "jj log post-processor",
		Long:  longHelp,
		Args:  cobra.ArbitraryArgs,
		// bijjou accepts an override for every config key, which cobra cannot
		// model. The raw arguments go to the config layer, and the command
		// answers help and version itself.
		DisableFlagParsing: true,
		RunE: func(cmd *cobra.Command, args []string) error {
			*code = run(cmd, args)
			return nil
		},
	}
	// These flags document the options the config layer reads. Flag parsing is
	// off, so they carry help text only.
	flags := cmd.Flags()
	flags.String("activate", "", "processing mode (auto|always|never); default always; bare flag = always")
	flags.Lookup("activate").NoOptDefVal = "always"
	flags.String("color", "", "color output (auto|always|never); default auto; bare flag = always")
	flags.Lookup("color").NoOptDefVal = "always"
	flags.String("stream", "", "streaming mode (default on); bare flag = true; --stream=false disables")
	flags.Lookup("stream").NoOptDefVal = "true"
	flags.String("<key>", "", "override any config key; replace '.' with '__'")
	return cmd
}

// run mirrors the startup sequence: help and version first, then the config
// from the file, the environment and the command line, then the pipeline.
func run(cmd *cobra.Command, args []string) int {
	for _, arg := range args {
		if arg == "-h" || arg == "--help" {
			if err := cmd.Help(); err != nil {
				return fail(err)
			}
			return 0
		}
	}
	for _, arg := range args {
		if arg == "-v" || arg == "--version" {
			fmt.Fprintf(cmd.OutOrStdout(), "%s version %s\n", cmd.Name(), cmd.Version)
			return 0
		}
	}

	cfg, err := config.Load()
	if err != nil {
		return fail(err)
	}
	if err := cfg.ApplyEnv(); err != nil {
		return fail(err)
	}
	if err := cfg.ApplyCLI(args); err != nil {
		return fail(err)
	}
	// A pager of "always" needs a pager to run. Say so before the input moves.
	if cfg.Pager == config.ModeAlways && strings.TrimSpace(os.Getenv("PAGER")) == "" {
		diag.Print(`bijjou: pager = "always" but PAGER env var is not set`)
		return 2
	}
	config.Init(cfg)

	if err := dispatch(); err != nil {
		// A closed pipe is the reader leaving early, not a failure.
		if errors.Is(err, syscall.EPIPE) {
			return 0
		}
		diag.Printf("Error: %v", err)
		return 1
	}
	return 0
}

// dispatch selects the path for this run: the raw copy, the streaming pass or
// the buffered pass.
func dispatch() error {
	c := config.Get()
	if c.Activate == config.ModeNever {
		if _, err := io.Copy(os.Stdout, os.Stdin); err != nil {
			return err
		}
		return nil
	}
	if c.StreamEnabled {
		return stream.Run()
	}
	return pipeline.RunBuffered()
}

// fail reports a configuration or usage error and gives the exit code for it.
func fail(err error) int {
	diag.Printf("bijjou: %v", err)
	return 2
}

// version reports the version of the main module, or "dev" for a build that
// carries none.
func version() string {
	if info, ok := debug.ReadBuildInfo(); ok {
		if v := info.Main.Version; v != "" && v != "(devel)" {
			return v
		}
	}
	return "dev"
}
