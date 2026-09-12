> Historical C/QBE implementation plan, superseded by Decision122. Use [the current design](../../DESIGN.md) and [build guide](../../BUILD.md).

## Implementation

Fern's compiler is implemented in **C with safety libraries**, targeting **QBE** for fast, small native binaries.

**Key Documents:**
- **FERN_STYLE.md** — Coding standards (TigerBeetle-inspired)
- **CLAUDE.md** — AI development workflow and TDD rules
- **ROADMAP.md** — Implementation milestones and tasks

### Compiler Language: Safe C

**Why C for AI-assisted development:**
- Massive training corpus (AI writes excellent C code)
- Fast iteration (compile and test in seconds)
- Direct control over memory and codegen
- Native performance

### FERN_STYLE Requirements

All compiler code follows **FERN_STYLE.md** (inspired by [TigerBeetle's TIGER_STYLE](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md)):

| Rule | Requirement |
|------|-------------|
| Assertion density | Minimum 2 assertions per function |
| Function size | Maximum 70 lines |
| Pair assertions | Assert before write AND after read |
| Bounds | Explicit limits on all loops/buffers |
| Memory | Arena allocation only (no malloc/free) |

**Why these rules:**
- Assertions catch bugs via fuzzing (force multiplier)
- Small functions fit in AI context windows
- Explicit bounds prevent infinite loops and overflows
- Arena allocation eliminates memory bugs

**Safety Stack (all MIT/BSD licensed):**

| Library | Purpose | Size | Safety Benefit |
|---------|---------|------|----------------|
| **tsoding/arena** | Memory allocation | ~300 lines | Eliminates use-after-free bugs |
| **Datatype99** | Tagged unions | ~1000 lines | Rust-like enums, exhaustive matching |
| **SDS** | String handling | ~1000 lines | Binary-safe, length-tracked (Redis) |
| **stb_ds.h** | Data structures | ~1500 lines | Hash maps, dynamic arrays |
| **safeclib** | Bounds checking | Optional | strcpy_s, memcpy_s, etc. |

**Total dependencies:** ~4,000 lines of third-party code

### Backend: QBE (Embedded)

QBE is embedded directly into the fern binary - no external `qbe` installation needed.

**Compilation Pipeline:**
```
Fern Source → Fern Compiler → QBE IR → [embedded QBE] → Assembly → [cc] → Native Binary
                    ↑                        ↑
            Single fern binary         No external qbe
```

**Why QBE:**
- ✅ Simple IR (easy for AI to generate)
- ✅ Fast compilation (< 1s for most programs)
- ✅ Small binaries (200-300 KB)
- ✅ Good performance (75-80% of LLVM, plenty for CLI tools)
- ✅ Embeddable (~6,650 lines of C, no dependencies)
- ✅ Single binary deployment (fern binary is self-contained)

**QBE IR Example:**
```qbe
export function w $add(w %a, w %b) {
@start
    %c =w add %a, %b
    ret %c
}
```

### Project Structure

```
fern/
├── src/              # Compiler (C with safety libraries)
│   ├── main.c        # CLI entry point
│   ├── lexer.c       # Tokenization
│   ├── parser.c      # Parse to AST (uses Datatype99)
│   ├── ast.h         # AST definitions (tagged unions)
│   ├── typecheck.c   # Type inference & checking
│   ├── codegen.c     # QBE IR generation
│   └── error.c       # Error reporting
│
├── runtime/          # Fern runtime (C)
│   ├── process.c     # Actor processes
│   ├── scheduler.c   # Work-stealing scheduler
│   ├── mailbox.c     # Type-safe message queues
│   └── libsql.c      # libSQL wrapper
│
├── stdlib/           # Standard library (Fern code)
│   ├── core.fn
│   ├── list.fn
│   ├── concurrent/   # Actor-based patterns
│   │   ├── cache.fn
│   │   └── queue.fn
│   └── db/
│       └── sql.fn
│
├── deps/             # Third-party libraries
│   ├── arena.h
│   ├── stb_ds.h
│   ├── datatype99.h
│   ├── sds.h
│   └── sds.c
│
└── tests/
    ├── test_lexer.c
    ├── test_parser.c
    └── e2e/
        └── *.fn
```

### Safe AST with Datatype99

**Tagged unions with exhaustive matching:**

```c
#include <datatype99.h>

// Expression types
datatype(
    Expr,
    (IntLit, int64_t),
    (StringLit, sds),
    (Ident, sds),
    (BinOp, struct Expr*, TokenType, struct Expr*),
    (Call, sds, struct Expr**),
    (Match, struct Expr*, struct MatchArm**)
);

// Pattern matching (compiler warns if cases missing!)
int64_t eval(Expr *expr) {
    match(*expr) {
        of(IntLit, n) return *n;
        of(BinOp, lhs, op, rhs) {
            int64_t left = eval(*lhs);
            int64_t right = eval(*rhs);
            switch (*op) {
                case TOK_PLUS: return left + right;
                case TOK_MINUS: return left - right;
                // ...
            }
        }
        of(Ident, name) return lookup_var(*name);
        // Compiler error if we miss any variant
    }
}
```

### Memory Management: Arena Allocation

**Never-free pattern (eliminates use-after-free):**

```c
#include "arena.h"

typedef struct {
    Arena ast_arena;     // AST nodes
    Arena string_arena;  // String interning
    Arena type_arena;    // Type information
} CompilerContext;

// Allocate from arena (no individual free needed)
Expr* new_expr(CompilerContext *ctx) {
    return arena_alloc(&ctx->ast_arena, sizeof(Expr));
}

// Free everything at once when compilation done
void ctx_free(CompilerContext *ctx) {
    arena_free(&ctx->ast_arena);
    arena_free(&ctx->string_arena);
    arena_free(&ctx->type_arena);
}
```

### Runtime Memory Management: Garbage Collection

The compiler uses arena allocation (above), but **compiled Fern programs** use automatic garbage collection:

**Current: Boehm GC**

```
┌─────────────────────────────────────────────────────────────┐
│  Fern Program Memory Model (v1)                             │
├─────────────────────────────────────────────────────────────┤
│  • Boehm conservative garbage collector                     │
│  • Statically linked - binaries are fully standalone        │
│  • No manual memory management needed                       │
│  • All allocations automatically reclaimed                  │
│  • Zero memory leaks guaranteed                             │
│  • Works seamlessly with C FFI                              │
└─────────────────────────────────────────────────────────────┘
```

**Why Boehm GC:**
- Drop-in replacement for malloc (~100 lines of integration)
- Proven in production (Mono, GCJ, many languages)
- Conservative scanning works with C interop
- No runtime pauses visible in benchmarks
- **Statically linked** - no runtime dependencies

**Installation (development only):**
```bash
# macOS
brew install bdw-gc

# Ubuntu/Debian
apt install libgc-dev

# Fedora
dnf install gc-devel
```

**Future: BEAM-Style Per-Process Heaps**

When actors are implemented (Milestone 8), Fern will transition to per-process garbage collection:

```
┌─────────────────────────────────────────────────────────────┐
│  Fern Actor Memory Model (future)                           │
├─────────────────────────────────────────────────────────────┤
│  Actor 1          Actor 2          Actor 3                  │
│  ┌─────────┐      ┌─────────┐      ┌─────────┐              │
│  │ Young   │      │ Young   │      │ Young   │              │
│  │ Heap    │      │ Heap    │      │ Heap    │              │
│  ├─────────┤      ├─────────┤      ├─────────┤              │
│  │ Old     │      │ Old     │      │ Old     │              │
│  │ Heap    │      │ Heap    │      │ Heap    │              │
│  └─────────┘      └─────────┘      └─────────┘              │
│       │                │                │                   │
│       └────── Messages copied between heaps ──────┘         │
└─────────────────────────────────────────────────────────────┘
```

**Benefits of per-process heaps:**
- GC never blocks other actors (isolated heaps)
- Process death = instant memory reclamation
- No global GC pauses
- BEAM-level latency guarantees

**Future: Perceus for WASM Support**

Boehm GC relies on stack scanning and OS features that don't work in WebAssembly. For WASM targets, Fern will use **Perceus-style reference counting**:

- **No cycles possible** - Fern's functional purity eliminates reference cycles
- **Zero annotations** - Unlike Rust, no lifetime annotations needed
- **"Functional but in-place"** - Compiler mutates unique values behind the scenes
- **Unified model** - Same memory strategy for native and WASM

See [docs/MEMORY_MANAGEMENT.md](docs/history/MEMORY_MANAGEMENT_PLAN.md) for the complete design.

### Error Handling: Result Types

**Explicit error handling with Datatype99:**

```c
datatype(
    ParseResult,
    (OkAst, Expr*),
    (ParseErr, sds)  // Error message
);

ParseResult parse_expr(Parser *p) {
    if (/* error condition */) {
        sds msg = sdscatprintf(sdsempty(),
            "%s:%d: Unexpected token", p->file, p->line);
        return ParseErr(msg);
    }

    Expr *expr = /* ... */;
    return OkAst(expr);
}

// Usage (must handle both cases)
ParseResult result = parse_expr(parser);
match(result) {
    of(OkAst, expr) {
        // Success path
        codegen(*expr);
    }
    of(ParseErr, msg) {
        // Error path
        fprintf(stderr, "%s\n", *msg);
        exit(1);
    }
}
```

### Development Workflow

**Debug builds:**
```bash
mise run debug

# Debug profile:
# - Symbols enabled
# - Assertions enabled
# - Strict warnings
```

**Release builds:**
```bash
mise run release

# Optimized, stripped, production-ready
```

### Implementation Milestones

These are the milestone acceptance criteria. Verified implementation status and
remaining work are recorded in [ROADMAP.md](ROADMAP.md) and
[release readiness](docs/RELEASE_READINESS.md).

**Milestone 1: Minimal Compiler**
- Lexer (keywords, identifiers, literals)
- Parser (functions, expressions, let bindings)
- Basic AST with Datatype99
- QBE codegen for simple programs
- Compile: `fn main() -> Int: 42`

**Milestone 2: Core Language**
- Type system (basic inference)
- Pattern matching (match expressions)
- Recursion (tail call optimization)
- Multiple function clauses
- Compile factorial, fibonacci, etc.

**Milestone 3: Type System**
- Generics (monomorphization)
- Traits (definition and implementation)
- Sum types (enums)
- Record types (structs)
- Type constraints (where clauses)
- Compile generic data structures

**Milestone 4: Standard Library**
- Collections (List, Map, Set)
- Option, Result types
- String operations
- File I/O
- Pipes (`|>` operator)
- List comprehensions

**Milestone 5: Actor Runtime**
- Spawn/send/receive primitives
- Work-stealing scheduler (C implementation)
- Mailboxes with type-safe Pid(msg)
- Supervision (spawn_link, monitor)
- Integration with QBE-generated code
- Stdlib actors (cache, queue, pubsub)

**Milestone 6: Database Integration**
- libSQL C bindings
- FFI from Fern to C
- db.sql module (open, execute, query)
- Migration runner
- Transaction support
- Complete REST API example

**Milestone 7: Production Polish**
- Error messages (source locations, hints)
- Optimization passes (dead code elimination)
- Binary size reduction (strip unused runtime)
- Cross-compilation (Linux, macOS, Windows)
- Documentation and examples
- Self-hosting (compiler compiles itself)

### Performance Targets

| Metric | CLI Mode | Server Mode | Status |
|--------|----------|-------------|--------|
| Binary Size | < 1 MB | 2-4 MB | Target |
| Startup Time | < 5ms | < 10ms | Target |
| Compilation | < 1s | < 3s | Target |
| Runtime Perf | 75% of LLVM | 75% of LLVM | Acceptable |
| Memory (idle) | < 10 MB | < 30 MB | Target |

### Comparison with Other Approaches

| Approach | Binary Size | Compile Time | Runtime Perf | AI Friendliness | Complexity |
|----------|-------------|--------------|--------------|-----------------|------------|
| **C + QBE** | **300 KB** | **< 1s** | **75%** | **⭐⭐⭐⭐⭐** | **Low** |
| Rust + Cranelift | 400 KB | 1-2s | 90% | ⭐⭐⭐ | Medium |
| Python + LLVM | 500 KB | 10s | 100% | ⭐⭐⭐⭐⭐ | Medium |
| Go (custom) | 2 MB | 2s | 85% | ⭐⭐⭐⭐ | Medium |
| Zig + QBE | 200 KB | < 1s | 75% | ⭐⭐ | Low |

**C + QBE wins** on: AI productivity, simplicity, binary size, compile speed.

---
