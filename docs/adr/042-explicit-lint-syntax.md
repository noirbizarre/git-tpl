# ADR-042: Compile all retained template syntax during lint

## Status

Accepted; extends ADR-034's previously deferred inherited syntax coverage.

## Context

MiniJinja loads partials lazily.
Registering them does not validate unimported, unreachable or shadowed declarations.
Leaf-only lint also missed syntax failures in inherited output.

## Decision

Lint explicitly compiles every effective output template and every retained partial declaration.
Inherited outputs use the render merge's origin, override and removal decisions.
Shadowed partial declarations are checked too, because the loader retains them for ancestor access.
Removed partial declarations are absent and are not checked.
Output binaries retain rendering's exemption.
Leaf findings are emitted once; inherited output and partial findings have deterministic path/declaration order.

## Consequences

Malformed retained syntax fails lint even if no answer set imports or renders it.
No other static analysis rule gains inherited scope, and no runtime data is loaded or fetched.
Resolved ancestor repositories are reused; syntax checking introduces no extra resolution or network access.
