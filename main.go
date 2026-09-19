// Command bijjou post-processes the output of `jj log`.
package main

import (
	"context"
	_ "embed"
	"os"

	"tangled.org/jjt.io/bijjou/internal/cli"
	"tangled.org/jjt.io/bijjou/internal/config"
)

// exampleTOML is the annotated config that bijjou writes when no config file
// is there.
//
//go:embed bijjou-config.toml
var exampleTOML string

func main() {
	config.ExampleTOML = exampleTOML
	os.Exit(cli.Execute(context.Background()))
}
