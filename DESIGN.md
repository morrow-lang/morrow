# Fern Language Design Document

*A statically-typed, functional language with Python aesthetics that compiles to single binaries*

**Name:** Fern  
**CLI tool:** `fern`  
**File extension:** `.fn`

## Active full-stack direction

Decision124 selects supervised native actors plus a reactive Fern application
compiled to WebAssembly, connected through a typed WebSocket protocol. Shared
Fern types and pure functions cross compilation targets; heaps, authority and
runtime handles do not. The server owns business state and the browser owns local
interaction and rendering. A first-party framework supplies the application
experience while small CLI programs retain a focused dependency surface.

Memory remains automatic. Actor-owned payload heaps and copied messages are
implemented; precise native root/layout coverage and fair resumable scheduling
remain open. The separate WASM backend supports scalar values, strings and bounded
aggregates with precise tracing. WasmGC will be evaluated before ABI stabilization; borrow
inference and reuse remain internal optimizations. The working
[web preview](docs/WEB_PREVIEW.md) executes a complete typed Fern model/update/view
through a generic Rust browser host and native Fern room actors on pinned worker
threads, with optional durable checkpoints. General application packaging,
preemptive scheduling and clustered ownership remain integration goals.
The [full-stack architecture](docs/FULL_STACK_ARCHITECTURE.md) defines the current
direction and supersedes conflicting older proposals below. The
[roadmap](ROADMAP.md) distinguishes implemented features from acceptance gates.

## Design Philosophy

### The Four Pillars

**1. Spark Joy** 🌿
For those familiar with functional programming, writing Fern should feel delightful. Pattern matching, pipes, and immutability aren't obstacles - they're tools that make code a pleasure to write and read.

**2. One Obvious Way**
There should be one clear, idiomatic way to accomplish any task. We actively avoid the "many ways to do it" trap. When you see Fern code, you know what it does. When you write Fern code, you don't agonize over which approach to use.

**3. No Surprises**
The language actively prevents the bugs that waste your afternoon debugging. No null pointer exceptions. No unhandled errors silently failing. No race conditions from shared mutable state. If your code compiles, it probably works.

**4. Jetpack Included**
Everything you need is built-in: actors for concurrency, embedded database, HTTP client/server, CLI tools, terminal UI. Like Bun or Elixir's ecosystem, but compiled to a single binary.

### What We Prevent

| Bug Class | How Fern Prevents It |
|-----------|---------------------|
| Null pointer exceptions | `Option` type, exhaustive pattern matching |
| Unhandled exceptions | `Result` type, compiler enforces handling |
| Race conditions | Actors with isolated heaps, no shared mutable state |
| Silent failures | Must explicitly handle all error cases |
| "Works on my machine" | Single binary, all dependencies included |
| Action at a distance | Immutability prevents spooky mutation |
| Unreachable managed values | Rust-owned tracing collection and actor-owned payload heaps; resource close and complete precise-root coverage remain explicit work |
| Buffer overflows | Bounds-checked collections, no raw pointers |

### Core Principles

- **Readable**: Python-like indentation, no braces, clear keywords
- **Functional-first**: Immutable by default, pure functions, pattern matching
- **Strongly typed**: Static types with inference - safety without verbosity
- **Single binary**: Go-style deployment, no runtime dependencies
- **Pragmatic**: We take the best ideas and make them work together
- **Dual-purpose**: Same language for CLI tools and full-stack applications

### Inspirations

| Language | What We Borrowed |
|----------|-----------------|
| **Python** | Indentation-based syntax, readability focus |
| **Gleam** | Type system, simplicity, Result/Option |
| **Elixir** | Pattern matching, multiple clauses, actors, pragmatism |
| **Go** | Single binary deployment, simplicity |
| **Rust** | `?` operator, Result types, memory safety mindset |
| **Erlang** | Lightweight processes, fault tolerance, "let it crash" |
| **Ruby** | Postfix conditionals, developer happiness |

---

## Dual-Purpose Design

Fern is designed to excel at **two distinct use cases** with the same language and syntax:

### 1. Fast CLI Tools
**Target: Sub-millisecond startup, < 1 MB binaries**

Perfect for command-line utilities that need to be:
- **Fast**: Compiled performance, instant startup
- **Small**: Single binary under 1 MB
- **Portable**: No runtime dependencies
- **Safe**: Type-checked at compile time

**Examples:**
- File processing tools (grep, find, jq alternatives)
- Dev tools (build systems, formatters, linters)
- System utilities (monitoring, automation)
- Data transformation pipelines

**Binary characteristics:**
- Size: 500 KB - 1 MB (core + minimal stdlib)
- Startup: < 5ms
- Memory: < 10 MB
- No actor runtime included (automatically stripped if unused)

### 2. Full-Stack Applications
**Target: Complete backend in one binary, < 4 MB**

Replace entire infrastructure stacks:
- **No Redis needed**: Built-in actor-based caching
- **No RabbitMQ needed**: Actor-based message queues
- **No separate database**: Embedded libSQL (SQLite-compatible)
- **Actor concurrency**: Lightweight processes for scalability

**Examples:**
- REST APIs and web services
- Real-time applications (WebSocket servers)
- Background job processors
- Microservices

**Binary characteristics:**
- Size: 2.5-4 MB (includes actor runtime ~400 KB + libSQL ~2 MB)
- Startup: < 10ms
- Memory: 20-100 MB under load
- Automatically includes runtime when `spawn()` or `sql.open()` is used

### Compilation Modes

The compiler automatically detects which mode to use based on code analysis:

**Detection Rules:**

1. **Actor Runtime** is included if:
   - Code calls `spawn()`, `spawn_link()`, `send()`, `receive`, or `call()`
   - Code imports from `concurrent.*` modules
   - Detection includes transitive dependencies (if imported module uses actors, runtime is included)

2. **libSQL Database** is included if:
   - Code calls `sql.open()`, `sql.connect()`, or other `db.sql` functions
   - Code imports `db.sql` module
   - Detection is whole-program (any usage anywhere triggers inclusion)

3. **Dead Code Elimination**:
   - Unused functions are not analyzed for actor/database usage
   - If `spawn()` is in unused function, actor runtime not included
   - Tree-shaking happens before mode detection

**Examples:**

```fern
# CLI Mode (minimal) - 600 KB
fn main() -> Result((), Error):
    let content = read_file("data.txt")?
    process(content)
    Ok(())
# No actors, no database → CLI mode
```

```fern
# Server Mode (actors only) - 900 KB
fn main() -> Result((), Error):
    let cache = spawn(cache_actor)  # Actor detected → include runtime
    http.serve(8080, handler(cache))
# Actor runtime included, no database
```

```fern
# Server Mode (database only) - 2.7 MB
fn main() -> Result((), Error):
    let db = sql.open("app.db")?  # Database detected → include libSQL
    http.serve(8080, handler(db))
# libSQL included, no actor runtime
```

```fern
# Server Mode (full) - 3.5 MB
fn main() -> Result((), Error):
    let db = sql.open("app.db")?      # Database detected
    let cache = spawn(cache_actor)    # Actors detected
    http.serve(8080, handler(db, cache))
# Both actor runtime and libSQL included
```

**Conditional usage (runtime still included):**
```fern
fn main() -> Result((), Error):
    let use_cache = env.get("USE_CACHE") == "true"
    
    # Even if this branch never executes, actor runtime is included
    # because compiler detects spawn() call exists
    let cache = if use_cache:
        Some(spawn(cache_actor))
    else:
        None
    
    serve(cache)
# Actor runtime IS included (conservative analysis)
```

**Manual override available:**
```bash
# Force minimal mode (fails if actors/db detected)
fern build --mode=cli tool.fn      

# Force full mode (includes everything)
fern build --mode=full tool.fn     

# Auto-detect (default, recommended)
fern build tool.fn                 

# Explicit control over components
fern build --no-actors tool.fn     # Fail if spawn() used
fern build --no-db tool.fn         # Fail if sql.open() used
```

### The Same Language, Two Scales

**Morning: Build a CLI tool**
```
# git-stats.fn - Analyze git repositories
fn main() -> Result((), Error):
    let repo = git.open(".")?
    let commits = repo.log()?
    
    commits
        |> group_by((c) -> c.author)
        |> map((author, commits) -> (author, commits.len()))
        |> sort_by_value()
        |> print_table()
    
    Ok(())

# Binary: 800 KB, instant execution
```

**Afternoon: Build a web service**
```
# api.fn - REST API with caching and database
fn main() -> Result((), Error):
    let db = sql.open("api.db")?
    let cache = spawn(cache_actor)
    let queue = spawn(job_queue)
    
    http.serve(8080, handler(db, cache, queue))

# Binary: 3 MB, handles thousands of connections
```

**Use the same:**
- Syntax and semantics
- Type system and traits
- Pattern matching and pipes
- Error handling with Result
- Standard library modules

### Performance Targets

| Metric | CLI Mode | Server Mode |
|--------|----------|-------------|
| Binary Size | 500 KB - 1 MB | 2.5 - 4 MB |
| Startup Time | < 5ms | < 10ms |
| Memory (Idle) | < 10 MB | < 30 MB |
| Memory (Load) | < 20 MB | 50-200 MB |
| Concurrency | Single-threaded | Thousands of actors |

### Comparison with Other Languages

**CLI Tools:**
```
Python + PyInstaller:    50+ MB (bundled interpreter)
Ruby:                    30+ MB (with gems)
Node.js:                 60+ MB (with runtime)
Go:                      2-5 MB
Rust:                    500 KB - 2 MB
Fern (CLI mode):         500 KB - 1 MB ✅
```

**Web Services:**
```
Node.js + Redis + DB:    200+ MB, 3+ services
Python + Celery + DB:    300+ MB, 4+ services
Go + Redis + DB:         50 MB, 3+ services
Elixir/Erlang:           15 MB, 1 service (but larger binary)
Fern (Server mode):      3 MB, 1 service ✅
```

---

## Basic Syntax

### Variables and Immutability

```
let name = "Niklas"
let count = 42
let items = [1, 2, 3]
```

All bindings are immutable by default. Rebinding is allowed (like Elixir):

```
let x = 1
let x = x + 1  # Creates new binding, doesn't mutate
```

### Comments and Documentation

**Single-line comments:**
```fern
# This is a regular comment
let x = 42  # inline comment
```

**Block comments:**
```fern
/*
This is a block comment
useful for temporarily disabling code
or writing longer explanations
*/
```

**Documentation comments:**
```fern
@doc """
Calculates the factorial of a number.

# Examples

```fern
factorial(5)  # => 120
factorial(0)  # => 1
```

# Arguments

- n: A non-negative integer

# Returns

The factorial of n
"""
pub fn factorial(n: Int) -> Int:
    match n:
        0 -> 1
        _ -> n * factorial(n - 1)
```

**Multi-line string literals:**
```fern
# Not a doc comment, just a string value
let poem = """
Roses are red
Violets are blue
"""
```

**Module documentation:**
```fern
module geometry.shapes

@moduledoc """
Two-dimensional shapes. See `Shape` and `area`.
"""
```

**Comment syntax rules:**
- `#` for single-line comments (like Python/Elixir)
- `/* */` for block comments
- `@doc """..."""` for function/type documentation (like Elixir's `@doc`)
- `@moduledoc """..."""` once per file, after the `module` line and before the
  first declaration (like Elixir's `@moduledoc`)
- `"""..."""` alone for multi-line strings
- Documentation is Markdown; inline code naming a declaration (`area`,
  `geometry.shapes.area`) becomes a cross-reference in generated sites
- Documentation appears in generated docs and LSP hover tooltips

**Doc tests (executable examples):**

Examples in `@doc` comments are automatically tested:

```fern
@doc """
Divides two numbers.

# Examples

```fern
divide(10, 2)  # => Ok(5)
divide(15, 3)  # => Ok(5)
divide(10, 0)  # => Err(DivideError.DivisionByZero)
```

# Errors

Returns `Err(DivisionByZero)` when divisor is zero.
"""
pub fn divide(a: Int, b: Int) -> Result(Int, DivideError):
    return Err(DivideError.DivisionByZero) if b == 0
    Ok(a / b)
```

**Running doc tests:**
```bash
# Test all examples in @doc comments
fern test --doc

# Test docs in specific file
fern test --doc src/math.fn

# Include in regular test run
fern test  # Runs both unit tests and doc tests
```

**Doc test assertions:**
```fern
@doc """
# Examples

```fern
# Test equality
add(2, 3)  # => 5

# Test Result patterns
divide(10, 2)  # => Ok(5)
divide(10, 0)  # => Err(_)  # Any Err

# Test Option patterns
find_user("alice")  # => Some(_)  # Any Some
find_user("bob")    # => None

# Multi-line tests
let result = complex_operation(
    input: "data",
    options: default_options
)
result  # => Ok(ExpectedOutput(...))
```
"""
```

**Benefits:**
- ✅ Examples always work (tested automatically)
- ✅ Documentation stays accurate
- ✅ Great for TDD (write examples first)
- ✅ AI learns from working examples
- ✅ Catches API changes that break examples

### String Interpolation

Native string interpolation without prefixes:

```
let name = "world"
let greeting = "Hello, {name}!"

let result = "The answer is {compute_answer()}"

# Multi-line strings
let doc = """
    Dear {recipient},
    
    Your order #{order_id} has shipped.
    """
```

### Functions

Basic function definition:

```fern
fn greet(name: String) -> String:
    "Hello, {name}!"
```

**Program Entry Point (`main`):**

Every Fern program requires a `main` function. The return type can be omitted or explicit:

```fern
# Shorthand: omit return type (defaults to Unit, auto-returns 0)
fn main():
    println("Hello, world!")

# Explicit: return an Int exit code
fn main() -> Int:
    println("Hello, world!")
    0

# With Result for error handling
fn main() -> Result((), Error):
    let content = read_file("config.txt")?
    println(content)
    Ok(())
```

The shorthand `fn main():` is equivalent to `fn main() -> (): 0` — it defaults to Unit return type and automatically returns exit code 0 (success). This special case applies **only to main()**.

**Labeled arguments:**

```fern
# Definition - labels match parameter names
fn connect(host: String, port: Int, timeout: Int, retry: Bool):
    # ...

# Call with labels (any order)
connect(
    port: 8080,
    host: "localhost",
    timeout: 5000,
    retry: true
)

# The unique String parameter may remain positional
connect("localhost", port: 8080, timeout: 5000, retry: true)

# Mix positional and labeled (positional must come first)
connect("localhost", port: 8080, timeout: 5000, retry: true)
```

**When labels are required:**

For a direct source-function call, an exact `Bool` parameter and every position
whose type repeats in the finalized declared function scheme require labels.
Distinct generic variables and distinct newtypes remain distinct even when a
particular call uses the same representation or concrete type. Type aliases are
expanded before this comparison. Structural function values, lambdas, runtime
APIs, compiler builtins and constructors retain positional calling conventions;
they reject labels rather than guessing parameter names.

Simple binding parameters use their names as labels. Pattern clauses can declare
an external name independently of local bindings, and every clause contributes to
one consistent external interface:

```fern
fn choose(enabled true: Bool) -> Int: 1
fn choose(enabled false: Bool) -> Int: 0
choose(enabled: true)

fn subtract(left: Int, right: Int) -> Int: left - right
9 |> subtract(right: 2, left: _)
```

Labels may reorder parameter binding, but argument effects run once in written
order. A pipe evaluates its left side first. Its placeholder must carry the
required label. Positional arguments must precede labeled arguments.

```fern
# Multiple parameters of same type - labels prevent confusion
fn send_message(from: String, to: String, subject: String, body: String):
    # ...

# Call must use labels for clarity (compiler enforces)
send_message(
    from: "alice@example.com",
    to: "bob@example.com",
    subject: "Hello",
    body: "How are you?"
)

# Bool parameters always require labels
fn fetch_data(cache: Bool, async: Bool):
    # ...

fetch_data(cache: true, async: false)  # Clear!
# fetch_data(true, false)  # ❌ Compile error - ambiguous
```

**Documentation with labeled arguments:**

```fern
@doc """
Connects to a server.

# Arguments

- host: Server hostname or IP address
- port: Server port number
- timeout: Connection timeout in milliseconds
- retry: Whether to retry on failure

# Examples

```fern
connect(host: "localhost", port: 8080, timeout: 5000, retry: true)
```
"""
pub fn connect(host: String, port: Int, timeout: Int, retry: Bool):
    # ...
```

Multiple clauses with pattern matching:

```fern
fn factorial(0: Int) -> Int:
    1

fn factorial(n: Int) -> Int:
    n * factorial(n - 1)
```

Anonymous functions:

```fern
let double = (x: Int) -> x * 2

items |> map((x) -> x * 2)
```

---

## Type System

### Basic Types

```
Int      # Signed 64-bit integer (-9,223,372,036,854,775,808 to 9,223,372,036,854,775,807)
Float    # IEEE 754 double-precision 64-bit floating point
String   # UTF-8 encoded text
Bool     # true or false
List(a)  # Immutable list of elements of type a
Map(k, v)  # Immutable map with keys of type k and values of type v
Set(a)     # Immutable unique elements; see docs/SETS.md for supported key types
Result(ok, err)  # Result type for error handling
Option(a)  # Optional value (Some(a) or None)
```

**Numeric Type Details:**

**Int (64-bit signed integer):**
- Range: -2^63 to 2^63-1
- Overflow behavior: Wrapping in all builds (Decision 55). Invalid integer division/remainder by zero and negative integer exponents produce a runtime diagnostic after deferred cleanup.
- Integer division: Truncates toward zero (`7 / 2 == 3`)
- Operations: `+`, `-`, `*`, `/`, `%`, `**` (power), bitwise ops

**Float (64-bit IEEE 754):**
- Precision: ~15-17 decimal digits
- Special values: `inf`, `-inf`, `NaN`
- Operations: `+`, `-`, `*`, `/`, `**` (power)
- Comparison: NaN != NaN (IEEE 754 standard)

**String (UTF-8):**
- Immutable
- Length in bytes (not characters/graphemes)
- Use `.chars()` for character iteration
- Use `.graphemes()` for grapheme cluster iteration

**Bool:**
- Values: `true`, `false`
- Operators: `and`, `or`, `not`, `==`, `!=`

**Generic Type Variables:**
- Type variables must be lowercase: `a`, `err`, `key`, `value`
- Concrete types are capitalized: `Int`, `String`, `User`
- This prevents ambiguity: `List(a)` is clearly generic, `List(User)` is concrete

**No Null:**
- Fern has no `null` or `nil` values
- Use `Option(a)` to represent optional values explicitly
- All values must be initialized
- The compiler guarantees no null pointer exceptions (except in C FFI boundary)

### Custom Types

Sum types (tagged unions):

```
type Status:
    Active
    Inactive
    Pending(since: DateTime)

type Result(ok, err):
    Ok(ok)
    Err(err)

type Option(a):
    Some(a)
    None
```

Record types:

```
type User derive(Show, Eq):
    name: String
    email: String
    age: Int

let user = User("Niklas", "niklas@example.com", 30)

# Access
user.name

# Update (returns new record)
let updated = %{ user | age: 31 }
```

### Union Types

Set-theoretic union types for flexible type composition (inspired by Elixir's type system):

```
fn handle(value: String | Int) -> String:
    match value:
        s: String -> "got string: {s}"
        n: Int -> "got number: {n}"

fn process(input: Ok(data) | Err(msg)) -> Result(Output, Error):
    match input:
        Ok(data) -> transform(data)
        Err(msg) -> Err(Error.from_string(msg))

# Useful for APIs that accept multiple types
fn log(level: Info | Warning | Error, message: String) -> ():
    # ...
```

### Type Aliases

Simple type aliases:

```
type UserId = Int
type Headers = Map(String, String)
```

### Newtypes

Newtypes create distinct types with compile-time guarantees and zero runtime cost:

```
newtype UserId = UserId(Int)
newtype ProductId = ProductId(Int)
newtype Email = Email(String)

let user_id = UserId(123)
let product_id = ProductId(123)

# Compile error: can't mix UserId and ProductId
fn get_user(id: UserId) -> User:
    # ...

get_user(product_id)  # ❌ Type error!
get_user(user_id)     # ✅ OK

# Access wrapped value
let raw_id: Int = user_id.0
```

### Traits

Traits define shared behavior across types:

```
trait Show(a):
    fn show(a) -> String

trait Eq(a):
    fn eq(a, a) -> Bool
    fn neq(a, a) -> Bool:
        not eq(a, a)

trait Ord(a) with Eq(a):  # Ord requires Eq
    fn compare(a, a) -> Ordering
```

Implementing traits:

```
type Point derive(Eq):
    x: Float
    y: Float

impl Show(Point):
    fn show(p: Point) -> String:
        "Point({p.x}, {p.y})"

# Manual Eq implementation (instead of derive)
impl Eq(Point):
    fn eq(p1: Point, p2: Point) -> Bool:
        p1.x == p2.x and p1.y == p2.y
```

Automatic trait derivation:

```
type User derive(Show, Eq, Ord):
    name: String
    age: Int

# Compiler generates implementations automatically:
# - Show: structural representation
# - Eq: field-by-field equality
# - Ord: lexicographic ordering
```

Commonly derivable traits:
- `Show`: string representation
- `Eq`: equality comparison
- `Ord`: ordering comparison
- `Clone`: deep copying (for types that need it)

### Generic Constraints

Use traits to constrain generic types:

```
fn sort(items: List(a)) -> List(a) where Ord(a):
    # Can use compare() on elements
    # ...

fn dedupe(items: List(a)) -> List(a) where Eq(a):
    # Can use eq() to compare elements
    # ...

fn display_all(items: List(a)) -> () where Show(a):
    for item in items:
        print(show(item))

# Multiple constraints
fn sort_and_display(items: List(a)) -> () where Ord(a), Show(a):
    items
        |> sort()
        |> display_all()
```

### Type Inference

Type annotations are required at API boundaries, optional internally:

```
# ✅ Required: public function signatures
pub fn process(input: String) -> Result(Int, Error):
    let trimmed = input.trim()      # inferred: String
    let parsed = parse_int(trimmed) # inferred: Result(Int, ParseError)
    parsed |> map((n) -> n * 2)     # return type checked

# ✅ Required: type definitions
type Config:
    timeout: Int
    retries: Int

# ✅ Optional: local variables (for clarity)
let users: List(User) = fetch_all()

# ❌ Not required: private functions (can be inferred)
fn internal_helper():
    let temp = compute()
    temp * 2

# Lambda parameters can often be inferred from context
items |> map((x) -> x * 2)  # x type inferred from items
```

**Inference Rules:**
1. Public function signatures must be annotated
2. Type definitions must be explicit
3. Local bindings can be inferred
4. Lambda parameters inferred from usage context
5. When in doubt, add annotations for clarity

---

## Pattern Matching

### Match Expression

Matching on values:

```
fn describe(value: Result(Int, String)) -> String:
    match value:
        Ok(0) -> "zero"
        Ok(n) if n > 0 -> "positive: {n}"
        Ok(n) -> "negative: {n}"
        Err(msg) -> "error: {msg}"
```

Multi-line match arms:

```
fn process(event: Event) -> Response:
    match event:
        Click(x, y) ->
            let adjusted = adjust_coords(x, y)
            emit_click(adjusted)
            Response.ok()
        
        KeyPress(key) if is_modifier(key) ->
            update_modifiers(key)
            Response.handled()
        
        _ -> Response.ignore()
```

### Condition Matching

`match` without a value for conditional branching:

```
let category = match:
    age < 13 -> "child"
    age < 20 -> "teenager"
    age < 65 -> "adult"
    _ -> "senior"
```

This replaces traditional `if`/`else if`/`else` chains.

### Postfix Guards

For early returns and simple conditionals:

```
fn divide(a: Int, b: Int) -> Result(Int, String):
    return Err("division by zero") if b == 0
    Ok(a / b)

fn process(items: List(Item)) -> List(Item):
    return [] if is_empty(items)
    # ... rest of processing
```

**Note:** Only `if` is supported for postfix guards. There is no `unless` keyword — use `if not` for negated conditions:

```fern
log("debug: {msg}") if debug_mode
skip_validation() if not strict_mode
```

### Destructuring

In let bindings:

```fern
let (x, y) = get_coordinates()
let User(name, email, _) = current_user
let [first, second, ..rest] = items
```

In function parameters:

```fern
fn distance((x1, y1): Point, (x2, y2): Point) -> Float:
    sqrt((x2 - x1) ** 2 + (y2 - y1) ** 2)
```

### Wildcard Patterns

Use `_` to ignore values you don't need.

**Ignore single value:**
```fern
# Tuple destructuring
let (x, _, z) = point3d  # Ignore y coordinate

# List destructuring
let [first, _, third] = items  # Ignore second element

# Type destructuring
let User(name, _, _) = user  # Only need name
```

**Refutable bindings:** A list length pattern can fail. Use `let pattern = value else: ...`
with a diverging else branch, or `match`, unless the pattern is irrefutable (such
as `[..all]`). This rule also applies to loop and with success bindings. The
plain-list examples below illustrate the pattern shape; potentially failing
uses require explicit failure handling (decision 60).

**Ignore remaining values:**
```fern
# List - collect rest
let [first, ..rest] = items
# first: first element
# rest: List with remaining elements

# List - ignore rest
let [first, .._] = items
# first: first element
# rest explicitly discarded

# Tuple - collect rest
let (head, ..tail) = tuple_value
# head: first element
# tail: Tuple with remaining elements

# Tuple - ignore rest
let (first, second, .._) = tuple_value
# first, second: used
# rest explicitly discarded
```

**In function parameters:**
```fern
# Ignore unused parameters
fn handler(_, value, _):
    process(value)

# Pattern match, ignore data
fn area(Circle(_)) -> Float:
    pi() * 5.0 * 5.0  # Fixed radius for example

fn area(Rectangle(w, h)) -> Float:
    w * h

# Ignore parts of destructured parameter
fn format_user((name, _, email): (String, Int, String)) -> String:
    "{name} <{email}>"  # Age ignored
```

**In match arms:**
```fern
# Ignore success value
match result:
    Ok(_) -> log("Success!")
    Err(e) -> log("Error: {e}")

# Ignore all fields of variant
match status:
    Active -> "active"
    Inactive -> "inactive"
    Pending(_) -> "pending"  # Ignore the timestamp

# Catch-all pattern
match value:
    1 -> "one"
    2 -> "two"
    _ -> "other"  # Matches anything
```

**Multiple wildcards:**
```fern
# Each _ is independent, not a binding
let (_, _, z) = point3d  # Ignore x and y

match coords:
    (0, 0, _) -> "on xy plane at origin"
    (_, 0, 0) -> "on xz plane at origin"
    (0, _, 0) -> "on yz plane at origin"
    _ -> "other position"
```

**Style guidelines:**
```fern
# ✅ Good: Use _ for truly unused values
let (x, _, z) = point
let [first, .._] = items

# ⚠️ Warning: Compiler warns about unused bindings
let (x, y, z) = point  # Warning: `y` is never used

# ✅ Good: Prefix with _ if might use later
let (_temp, value) = compute()  # _temp documented as unused

# ❌ Bad: Binding then ignoring
let (x, y, z) = point
# ... only use x and z (y should be _)
```

**Difference between `..rest` and `.._`:**
```fern
# ..rest - bind the rest (can use it)
let [first, ..rest] = [1, 2, 3, 4]
print(rest)  # [2, 3, 4]

# .._ - ignore the rest (cannot use it)
let [first, .._] = [1, 2, 3, 4]
# rest is not bound, cannot be used
```

**Compiler behavior:**
- `_` never creates a binding
- Compiler doesn't check if `_` is used (because it can't be)
- Unused named bindings generate warnings
- `_` can appear multiple times in same pattern
- `.._` vs `..name` affects whether compiler checks rest elements

---

## Pipe Operators

### Standard Pipe

Value goes to first argument position:

```
users
    |> filter(is_active)
    |> map(get_name)
    |> sort()
    |> take(10)
```

### Placeholder Pipe

Use `_` to specify argument position:

```
data
    |> transform(config, _)
    |> insert_at(collection, 0, _)
    |> write_to(_, filepath)
```

This unifies Clojure's `->` and `->>` into a single, more flexible operator.

### Chaining Example

```
fn process_order(order_id: Int) -> Result(Receipt, Error):
    order_id
        |> fetch_order()
        |> validate(_)
        |> apply_discount(current_promotions(), _)
        |> calculate_tax(_)
        |> generate_receipt()
```

---

## Control Flow

### Match (primary conditional)

```
# Condition-based
let result = match:
    x > 0 -> "positive"
    x < 0 -> "negative"
    _ -> "zero"

# Value-based
let result = match status:
    Active -> handle_active()
    Inactive -> handle_inactive()
    Pending(since) -> handle_pending(since)
```

### Postfix Conditionals

```
return None if is_empty(list)
log("debug: {msg}") if debug_mode
skip_validation() if not strict_mode
```

### Let-else (for early unwrapping)

```
fn process(input: Option(Data)) -> Result(Output, Error):
    let Some(data) = input else:
        return Err("no input provided")
    
    # Continue with data...
```

### Iteration

Fern provides `for` loops for iterating over collections. For stateful iteration, use recursion or functional combinators.

**For loops (iteration over collections):**
```fern
# Iterate over list
for item in items:
    process(item)

# With index (enumerate)
for (i, item) in items.enumerate():
    print("{i}: {item}")

# Iterate over range
for i in 0..10:
    print(i)  # Prints 0 through 9

# Inclusive range
for i in 0..=10:
    print(i)  # Prints 0 through 10

# Iterate over map
for (key, value) in user_map:
    print("{key}: {value}")
```

**Loop control:**
```fern
# Early exit with break
for item in items:
    break if item.is_target()
    process(item)

# Skip iteration with continue
for item in items:
    continue if item.should_skip()
    process(item)

# Return from loop
for item in items:
    return Some(item) if item.matches(criteria)
None  # No match found
```

**Functional alternatives (preferred):**
```fern
# Summing - use fold
let sum = items |> list.fold(0, (acc, x) -> acc + x)

# Finding - use find
let found = items |> list.find((x) -> x.matches(criteria))

# Transforming - use map
let doubled = items |> map((x) -> x * 2)

# Filtering - use filter
let evens = items |> filter((x) -> x % 2 == 0)
```

**Stateful iteration - use recursion:**
```fern
# Instead of while loops, use tail-recursive functions
fn count_down(n: Int) -> ():
    return () if n <= 0
    print(n)
    count_down(n - 1)  # Tail call optimized

# With accumulator
fn sum_list(items: List(Int)) -> Int:
    sum_loop(items, 0)

fn sum_loop([], acc) -> acc
fn sum_loop([x, ..xs], acc) -> sum_loop(xs, acc + x)
```

**Why no `while` or `loop`?**

Fern follows Gleam's approach: no `while` or `loop` constructs. These require mutable state between iterations, which conflicts with immutability. Recursion with tail-call optimization handles all stateful iteration patterns cleanly. This keeps the language simple and the semantics clear.

---

## Modules and Imports

### Module Declaration

Every file is a module. The module name matches the file path:

```fern
# File: math/geometry.fn
module math.geometry

# File: http/server.fn  
module http.server

# File: main.fn
module main
```

### Visibility Modifiers

**Public (`pub`) - visible to other modules:**
```fern
pub fn public_function() -> Int:
    42

pub type PublicType:
    field: String

pub trait PublicTrait(a):
    fn method(a) -> String
```

**Private (default) - visible only within module:**
```fern
# No pub keyword = private
fn private_helper() -> Int:
    internal_calculation()

type InternalType:
    secret: String

fn internal_calculation() -> Int:
    # Only usable within this module
    123
```

**Visibility rules:**
- Functions, types, and traits are private by default
- Use `pub` to make them public
- Private items cannot be accessed from outside the module
- Private items can use other private items in same module

### Import Syntax

**Import entire module:**
```fern
import math.geometry

# Use with qualified names
let area = math.geometry.area(shape)
```

**Import specific items:**
```fern
import math.geometry.{area, perimeter, volume}

# Use directly
let area = area(shape)
```

**Import with alias:**
```fern
import math.geometry as geo

let area = geo.area(shape)
```

**Import from nested modules:**
```fern
import http.server.middleware
import http.server.middleware.{cors, auth, logging}
```

**Import everything (use sparingly):**
```fern
import math.geometry.*

# All public items available directly
let area = area(shape)
let perimeter = perimeter(shape)
```

### Re-exports

Make imported items available to module users:

```fern
# File: http/mod.fn
module http

# Re-export from submodules
pub import http.client.{get, post, put, delete}
pub import http.server.{serve, Response, Request}

# Now users can do:
# import http.{get, serve}
```

### Module Organization

**Recommended structure:**
```
src/
├─ main.fn              # Entry point (module main)
├─ lib.fn               # Library root (module lib)
├─ utils/
│  ├─ string.fn         # module utils.string
│  ├─ io.fn             # module utils.io
│  └─ mod.fn            # module utils (re-exports)
└─ models/
   ├─ user.fn           # module models.user
   ├─ post.fn           # module models.post
   └─ mod.fn            # module models (re-exports)
```

**mod.fn pattern (re-export hub):**
```fern
# File: models/mod.fn
module models

pub import models.user.{User, UserId, create_user}
pub import models.post.{Post, PostId, create_post}

# Users can now do:
# import models.{User, Post}
```

### Circular Dependencies

**Not allowed - compile error:**
```fern
# File: a.fn
module a
import b

fn use_b() -> b.Value:
    b.create()

# File: b.fn  
module b
import a  # ERROR: Circular dependency

fn use_a() -> a.Value:
    a.create()
```

**Solution - extract shared module:**
```fern
# File: shared.fn
module shared
pub type Value:
    data: String

# File: a.fn
module a
import shared

fn create() -> shared.Value:
    shared.Value("from a")

# File: b.fn
module b
import shared

fn create() -> shared.Value:
    shared.Value("from b")
```

### Import Resolution

**Imports resolve from project root:**
```fern
# File: src/utils/string.fn
import models.user  # Resolves to src/models/user.fn
import http.client  # Resolves to stdlib http/client.fn (if not found in src/)
```

**Resolution order:**
1. Project source directory
2. Standard library
3. Dependencies (from fern.toml)

### Standard Library Imports

Standard library modules are available without prefix:

```fern
import list       # stdlib/core/list.fn
import json       # stdlib/json.fn
import http.client  # stdlib/http/client.fn
```

### Example: Complete Module

```fern
# File: models/user.fn
module models.user

import db.sql
import datetime.{DateTime, now}

# Public type
pub type User derive(Show, Eq):
    id: UserId
    name: String
    email: String
    created_at: DateTime

# Public newtype
pub newtype UserId = UserId(Int)

# Public function
@doc """
Creates a new user in the database.

Returns the created user with generated ID.
"""
pub fn create(db: Database, name: String, email: String) -> Result(User, Error):
    validate_email(email)?
    
    let id = insert_user(db, name, email)?
    
    Ok(User(
        id: UserId(id),
        name: name,
        email: email,
        created_at: now()
    ))

# Public function
pub fn find_by_id(db: Database, id: UserId) -> Result(Option(User), Error):
    let UserId(raw_id) = id
    query_user(db, raw_id)

# Private helper - not visible outside module
fn validate_email(email: String) -> Result((), Error):
    return Err(Error.invalid("email", "must contain @")) if not email.contains("@")
    Ok(())

# Private helper
fn insert_user(db: Database, name: String, email: String) -> Result(Int, Error):
    sql.execute(db, 
        "INSERT INTO users (name, email) VALUES (?, ?)",
        [name, email]
    )?
    Ok(sql.last_insert_id(db))

# Private helper
fn query_user(db: Database, id: Int) -> Result(Option(User), Error):
    let rows = sql.query(db, "SELECT * FROM users WHERE id = ?", [id])?
    match rows:
        [] -> Ok(None)
        [row] -> Ok(Some(parse_user(row)?))
        _ -> Err(Error.unexpected("multiple users with same id"))

fn parse_user(row: Row) -> Result(User, Error):
    Ok(User(
        id: UserId(row.get_int("id")?),
        name: row.get_string("name")?,
        email: row.get_string("email")?,
        created_at: row.get_datetime("created_at")?
    ))
```

**Using the module:**
```fern
# File: main.fn
module main

import models.user.{User, UserId, create, find_by_id}
import db.sql

fn main() -> Result((), Error):
    let db = sql.open("app.db")?
    
    # Create user
    let user = create(db, "Alice", "alice@example.com")?
    print("Created user: {user.name}")
    
    # Find user
    match find_by_id(db, user.id)?:
        Some(found) -> print("Found user: {found.name}")
        None -> print("User not found")
    
    Ok(())
```

---

## Error Handling

Fern uses explicit error handling with the `Result` type. **Errors must be handled** - the compiler enforces this.

### Result Type

All functions that can fail return `Result`:

```fern
fn read_file(path: String) -> Result(String, IoError):
    # ...

fn parse_config(content: String) -> Result(Config, ParseError):
    # ...

fn divide(a: Int, b: Int) -> Result(Int, DivideError):
    return Err(DivideError.DivisionByZero) if b == 0
    Ok(a / b)
```

### Errors Must Be Handled

**Compiler enforcement:**

```fern
fn main() -> Result((), Error):
    let content = read_file("config.txt")  # ❌ Compile error!
    # Error: Result value must be handled
    #
    # Help: Use one of:
    #   - Propagate with ?: let content = read_file("config.txt")?
    #   - Match: match read_file(...): Ok(c) -> ..., Err(e) -> ...
    #   - Combinator: read_file(...) |> unwrap_or(default)

    # ✅ Correct: Use ? to propagate error
    let content = read_file("config.txt")?
    process(content)
    Ok(())
```

**The compiler will reject:**
- Ignoring Result return values
- Using Result values without pattern matching or `?`
- Unhandled error cases

**Valid handling strategies:**

```fern
# 1. Use ? operator (propagate error up) - MOST COMMON
let value = fallible_operation()?

# 2. Pattern match (custom error handling)
match fallible_operation():
    Ok(value) -> use_value(value)
    Err(e) -> handle_error(e)

# 3. Use combinators (transform/provide defaults)
let result = fallible_operation()
    |> map((v) -> transform(v))
    |> unwrap_or(default_value)

# 4. with expression (complex cases with multiple Results)
with
    x <- operation1(),
    y <- operation2(x)
do
    Ok(y)
else
    Err(e) -> handle(e)
```

### The `?` Operator (Result Propagation)

Unwrap Result or early return on error:

```fern
fn load_and_process(path: String) -> Result(Output, Error):
    let content = read_file(path)?          # Returns Err if fails
    let config = parse_config(content)?     # Returns Err if fails
    let validated = validate(config)?       # Returns Err if fails
    Ok(process(validated))                  # All succeeded
```

**How `?` works:**
- If `Ok(value)`, evaluates to `value` and continues
- If `Err(e)`, immediately returns `Err(e)` from function
- Can only be used in functions returning `Result`
- Postfix operator - attached to the expression that might fail

**Why `?` (Rust-style):**
- ✅ Familiar from Rust - widely understood
- ✅ Postfix shows WHAT can fail - you see the call first, then the `?`
- ✅ Compact - doesn't require separate binding syntax
- ✅ Chainable - `foo()?.bar()?.baz()?`
- ✅ Works naturally with `let` bindings

### The `with` Expression (Complex Error Handling)

For cases where you need custom error handling for different operations, use `with`. This is the **only** place where `<-` appears in Fern — it binds Results within a `with` block.

```fern
fn process_request(req: Request) -> Result(Response, Error):
    with
        user <- authenticate(req),
        permissions <- get_permissions(user),
        data <- fetch_data(req.path),
        validated <- validate_access(permissions, data)
    do
        # Success path - all operations succeeded
        Ok(Response.ok(validated))
    else
        # Error handling with pattern matching
        Err(AuthError(msg)) ->
            log.warn("Auth failed: {msg}")
            Ok(Response.unauthorized())
        Err(PermissionError(msg)) ->
            log.warn("Permission denied: {msg}")
            Ok(Response.forbidden())
        Err(DataError(msg)) ->
            log.error("Data error: {msg}")
            Ok(Response.not_found())
        Err(e) ->
            log.error("Unexpected error: {e}")
            Ok(Response.error(500, "Internal error"))
```

**How `with` works:**
1. Evaluates each `<-` binding in sequence
2. If all succeed, executes `do` block
3. If any fails, jumps to `else` block with the error
4. Can pattern match on different error types
5. Must return same type from all branches

**When to use `with` vs `?`:**
- Use `?` when errors should propagate up (most common)
- Use `with` when you need custom handling per error type
- Use `with` when operations are related and should be grouped

### Chaining with Pipes

```fern
fn load_config(path: String) -> Result(Config, Error):
    path
        |> read_file()          # Result(String, Error)
        |> and_then(parse_config)  # Result(Config, Error)
        |> map(validate_config)    # Result(Config, Error)
```

### The `defer` Statement (Resource Cleanup)

Ensure cleanup code runs when function exits (success or error):

```fern
fn process_file(path: String) -> Result(Data, Error):
    let file = open_file(path)?
    defer close_file(file)  # Guaranteed to run on exit

    let data = read_data(file)?
    let transformed = transform(data)?
    Ok(transformed)
    # close_file(file) is called here automatically
```

**How `defer` works:**
- Deferred code runs when function exits (any path)
- Runs in reverse order if multiple defers
- Runs even on early returns or errors
- Cannot access return value

**Multiple defers:**
```fern
fn complex_operation() -> Result(Output, Error):
    let resource1 = acquire_resource1()?
    defer release_resource1(resource1)  # Runs last

    let resource2 = acquire_resource2()?
    defer release_resource2(resource2)  # Runs first

    let result = compute(resource1, resource2)?

    # On exit (any path):
    # 1. release_resource2(resource2)
    # 2. release_resource1(resource1)

    Ok(result)
```

**With error handling:**
```fern
fn read_and_process(path: String) -> Result(Output, Error):
    let file = open_file(path)?
    defer close_file(file)

    # Even if this fails, close_file still runs
    let data = read_all(file)?

    Ok(process(data))
    # close_file runs here (success or error)
```

**Restrictions:**
- `defer` only in non-actor functions (actors have different lifecycle)
- Deferred code cannot fail (must be `-> ()`, not `-> Result`)
- Deferred code cannot use `<-` or `return`

**Why not in actors?**
Actors run forever in a loop - no clear "exit" point:
```fern
fn actor_loop():
    receive:
        Message(data) ->
            # defer doesn't make sense here
            actor_loop()  # Tail recursive, never exits
```

For actors, use manual cleanup or wrapper functions.

### Result Combinators

```fern
# map - transform success value
result |> map((x) -> x * 2)

# and_then - chain operations that return Result
result |> and_then((x) -> compute(x))

# or_else - handle error, try alternative
result |> or_else((e) -> fallback())

# unwrap_or - provide default on error
result |> unwrap_or(default_value)

# unwrap_or_else - compute default on error
result |> unwrap_or_else((e) -> compute_default(e))
```

### No Exceptions

Fern has **no exceptions** or try/catch:

```fern
# ❌ Not in Fern
try:
    risky_operation()
catch e:
    handle(e)

# ✅ Use Result instead
match risky_operation():
    Ok(value) -> process(value)
    Err(e) -> handle(e)
```

**Benefits:**
- ✅ Errors are visible in function signatures
- ✅ Compiler forces you to handle errors
- ✅ No hidden control flow
- ✅ No forgotten error handling
- ✅ Easier to reason about code
- ✅ **Programs never crash** - all errors are recoverable

### No Panics, No Crashes

**Fern has no panic mechanism.** Programs never crash from error conditions.

```fern
# ❌ Not in Fern - no panic!
panic("something went wrong")
result.unwrap()  # This method doesn't exist

# ✅ Always use Result for errors
fn compute(x: Int) -> Result(Int, ComputeError):
    return Err(ComputeError.InvalidInput) if x < 0
    Ok(x * 2)

# ✅ Provide defaults for "impossible" cases
let value = result.unwrap_or(default)
let value = result.unwrap_or_else((e) -> {
    log.error("Unexpected error: {e}")
    compute_fallback()
})

# ✅ Or handle explicitly
match result:
    Ok(v) -> v
    Err(e) -> 
        log.error("Error: {e}")
        default_value
```

**For "should never happen" cases:**

Instead of panicking, return a Result and handle gracefully:

```fern
# ❌ Don't do this (panic)
fn get_item(list: List(a), index: Int) -> a:
    panic("index out of bounds") if index >= list.len()
    # ...

# ✅ Do this (Result)
fn get_item(list: List(a), index: Int) -> Result(a, IndexError):
    return Err(IndexError.OutOfBounds(index, list.len())) if index >= list.len()
    Ok(list.internal_get(index))

# Caller decides what to do
match list.get(5):
    Ok(item) -> process(item)
    Err(IndexError.OutOfBounds(i, len)) ->
        log.warn("Index {i} out of bounds (len={len}), using default")
        process(default_item)
```

**Why no panics?**

1. **Predictability** - Programs never crash unexpectedly
2. **Actor safety** - Actor crashes would lose messages and state
3. **Server reliability** - Servers stay up, log errors, continue serving
4. **Better debugging** - Errors are logged and traceable, not just crashes
5. **Graceful degradation** - Applications can fall back to safe defaults
6. **No special cases** - All errors handled the same way

**What about programming bugs?**

Use assertions during development:

```fern
# Development/debug mode only
fn internal_check(condition: Bool, msg: String) -> ():
    # In debug builds, logs error and exits
    # In release builds, this is optimized out
    debug_assert(condition, msg)

fn process(data: Data) -> Result(Output, Error):
    debug_assert(data.is_valid(), "data should be validated before this")
    # ... continue processing
```

**Benefits of no-panic design:**

- ✅ Web servers never crash from errors
- ✅ CLI tools always exit cleanly
- ✅ Actors survive errors and keep processing
- ✅ No lost data from crashes
- ✅ Errors are always handleable
- ✅ Better user experience
- ✅ Simpler mental model (only Result, no panic paths)

### Error Type Design

**Define custom error types:**

```fern
type FileError:
    NotFound(String)      # Path
    PermissionDenied(String)
    InvalidFormat(String)
    IoError(String)

type ApiError:
    NetworkError(String)
    Unauthorized
    NotFound
    ServerError(Int, String)  # Status code, message

# Use in functions
fn load_file(path: String) -> Result(String, FileError):
    # ...
```

**Error conversion:**

```fern
# Convert specific error to general error
fn load_config() -> Result(Config, Error):
    let content = read_file("config.txt")
        |> map_err((e) -> Error.from_file_error(e))?
    
    parse_config(content)
        |> map_err((e) -> Error.from_parse_error(e))
```

---

## Collections

### Lists

```
let numbers = [1, 2, 3, 4, 5]

# Operations (all return new lists)
numbers |> append(6)
numbers |> prepend(0)
numbers |> concat([6, 7, 8])

# Comprehensions
let squares = [x * x for x in numbers]
let evens = [x for x in numbers if x % 2 == 0]
```

### Maps

```
let headers = %{
    "Content-Type": "application/json",
    "Accept": "application/json"
}

headers |> get("Content-Type")  # -> Some("application/json")
headers |> put("Authorization", token)
```

### Tuples

Tuples are fixed-size, positional collections:

```fern
let point = (10, 20)
let (x, y) = point

# Access by position
point.0  # 10
point.1  # 20

# Tuples can hold different types
let mixed = ("hello", 42, true)
```

**For named fields, use records (declared with `type`):**

```fern
type Point:
    x: Int
    y: Int

let point = Point(10, 20)
point.x  # 10
```

---

## Concurrency

Fern uses an **actor-based concurrency model** inspired by Erlang/Elixir, with type-safe message passing.

**Implementation boundary:** the default Rust frontend executes the bounded native
subset in [the native actor contract](docs/RUST_ACTORS.md), now with actor-owned
payload heaps, copied messages, bounded typed supervision and recursive Unit-tail
continuations. The web host pins independent room runtimes to multiple threads.
Generalized resumable scheduling, per-invocation multicore execution and complete
actor REPL/FernSim parity remain planned. The old C reference implementation has
been removed.

### Lightweight Processes

Spawn isolated processes (actors) that communicate via messages:

```
# Spawn a process
let pid = spawn(fn -> worker_loop())

# Spawn with a link (for supervision)
let pid = spawn_link(fn -> supervised_worker())

# Send messages (always immutable)
send(pid, Request("get", "/users"))

# Receive messages with pattern matching
fn worker_loop() -> ():
    receive:
        Request(method, path) ->
            let response = handle_request(method, path)
            worker_loop()  # Tail recursive
        
        Shutdown ->
            cleanup()
            return ()
        
        _ after 5000 ->
            # Timeout after 5 seconds
            handle_timeout()
            worker_loop()
```

### Type-Safe Message Passing

Process IDs are typed by the messages they accept:

```fern
type Pid(msg)  # A process that accepts messages of type msg

type CacheMsg(k, v):
    Get(k, Pid(Option(v)))        # Request value, reply to sender
    Set(k, v)                      # Store value
    Delete(k)                      # Remove value

fn cache_actor() -> ():
    let state = Map.new()
    cache_loop(state)

fn cache_loop(state: Map(k, v)) -> ():
    receive:
        Get(key, reply_to) ->
            let value = Map.get(state, key)
            send(reply_to, value)
            cache_loop(state)
        
        Set(key, value) ->
            let new_state = Map.put(state, key, value)
            cache_loop(new_state)
        
        Delete(key) ->
            let new_state = Map.delete(state, key)
            cache_loop(new_state)

# Usage
let cache: Pid(CacheMsg(String, User)) = spawn(cache_actor)

send(cache, Set("user:123", user_data))   # ✅ OK
send(cache, "invalid message")             # ❌ Type error!
```

**Type Safety Guarantees:**

1. **Compile-time checking**: The type system ensures you can only send messages of the correct type to a Pid
   - `Pid(CacheMsg(String, User))` only accepts `CacheMsg(String, User)` variants
   - Sending wrong type causes compile error

2. **Runtime behavior**: Messages are still dynamically dispatched
   - Actors use pattern matching in `receive` to handle messages
   - If receive pattern is incomplete, unhandled messages are queued (or dropped in strict mode)

3. **Type safety limits**:
   - Type safety enforced at send site (compile-time)
   - Pattern matching at receive site (runtime)
   - Doesn't prevent logic errors (e.g., sending Get when actor expects Set next)
   - No session types or protocol enforcement (planned for v2)

**Example of compile-time safety:**
```fern
let cache: Pid(CacheMsg(String, Int)) = spawn(cache_actor)

send(cache, Set("count", 42))        # ✅ OK - correct type
send(cache, Set("name", "Alice"))    # ❌ Type error - String not Int
send(cache, "hello")                 # ❌ Type error - not a CacheMsg

# Type system prevents these errors at compile time
```

### Synchronous Calls

For request-response patterns, use `call`:

```
# call waits for a reply with timeout
fn get_cached(cache: Pid(CacheMsg(k, v)), key: k) -> Result(Option(v), Error):
    call(cache, Get(key, _), timeout: 5000)

# Under the hood, call:
# 1. Spawns a temporary mailbox
# 2. Sends Get(key, mailbox_pid)
# 3. Waits for reply
# 4. Returns result or timeout error
```

### Supervision

Processes can monitor and restart each other:

```
fn supervisor() -> ():
    let worker = spawn_link(worker_loop)
    
    receive:
        Exit(pid, reason) if pid == worker ->
            # Worker crashed, restart it
            log("Worker crashed: {reason}, restarting...")
            let new_worker = spawn_link(worker_loop)
            # Continue supervising
            supervisor()

# If worker crashes, supervisor receives Exit message
# Supervisor can decide: restart, escalate, or terminate
```

### Actor Examples

**Example 1: In-Memory Cache**

```
import concurrent.cache

fn main() -> Result((), Error):
    # Spawn cache with 5-minute TTL
    let cache = cache.new(ttl: minutes(5))
    
    # Store value
    cache.set("session:abc", session_data)
    
    # Retrieve value
    match cache.get("session:abc"):
        Some(data) -> use_session(data)
        None -> create_new_session()
    
    Ok(())
```

**Example 2: Job Queue**

```
import concurrent.queue

fn main() -> Result((), Error):
    # Spawn queue with 10 workers
    let queue = queue.new(workers: 10)
    
    # Enqueue work
    for order in pending_orders():
        queue.enqueue(process_order(order))
    
    # Workers automatically process jobs concurrently
    
    Ok(())
```

**Example 3: WebSocket Connection Pool**

```
type ConnectionPool:
    connections: Map(UserId, Pid(Message))

fn connection_pool() -> ():
    pool_loop(ConnectionPool(Map.new()))

fn pool_loop(state: ConnectionPool) -> ():
    receive:
        Register(user_id, conn_pid) ->
            let new_conns = Map.put(state.connections, user_id, conn_pid)
            pool_loop(ConnectionPool(new_conns))
        
        Broadcast(message) ->
            # Send to all connected users
            for (_, pid) in state.connections:
                send(pid, message)
            pool_loop(state)
        
        SendTo(user_id, message) ->
            # Send to specific user
            match Map.get(state.connections, user_id):
                Some(pid) -> send(pid, message)
                None -> log("User {user_id} not connected")
            pool_loop(state)

# Usage
let pool = spawn(connection_pool)
send(pool, Register(user_id, connection))
send(pool, Broadcast(Notification("Server restarting")))
```

### Concurrency Primitives

The planned standard library will package these common patterns. The following
APIs are architectural sketches, not the current executable module reference:

```
# stdlib/concurrent/cache.fn
pub fn new(ttl: Duration) -> Pid(CacheMsg(k, v))
pub fn get(cache: Pid(CacheMsg(k, v)), key: k) -> Option(v)
pub fn set(cache: Pid(CacheMsg(k, v)), key: k, value: v) -> ()

# stdlib/concurrent/queue.fn
pub fn new(workers: Int) -> Pid(QueueMsg(job))
pub fn enqueue(queue: Pid(QueueMsg(job)), job: job) -> ()

# stdlib/concurrent/pubsub.fn
pub fn new() -> Pid(PubSubMsg(topic, msg))
pub fn subscribe(pubsub: Pid(PubSubMsg(topic, msg)), topic: topic) -> Pid(msg)
pub fn publish(pubsub: Pid(PubSubMsg(topic, msg)), topic: topic, msg: msg) -> ()

# stdlib/concurrent/pool.fn
pub fn new(size: Int, worker: fn() -> ()) -> Pid(PoolMsg)
pub fn checkout(pool: Pid(PoolMsg)) -> Result(Worker, Error)
pub fn checkin(pool: Pid(PoolMsg), worker: Worker) -> ()
```

### Benefits of Actor Model

1. **No shared memory** - eliminates data races
2. **Type-safe messages** - compile-time guarantees
3. **Fault tolerance** - isolated failures, supervision
4. **Scalability** - thousands of lightweight processes
5. **Natural fit** - works perfectly with immutability
6. **Replace infrastructure** - actors can replace Redis, queues, etc.

### Performance Characteristics

- **Process spawn**: ~1-2 μs per process
- **Message send**: ~100-200 ns for local messages
- **Memory per process**: ~2 KB base overhead
- **Max concurrent processes**: Hundreds of thousands (limited by memory)
- **Scheduler**: Work-stealing, preemptive multitasking

### When NOT to Use Actors

For pure computation with no I/O or state:

```
# Don't use actors for CPU-bound parallel work
# Use parallel_map instead (simpler API)
let results = parallel_map(items, expensive_computation)

# Under the hood: spawns N processes, distributes work, collects results
```

---

## Embedded Database

Fern includes **libSQL** (a SQLite-compatible embedded database) for zero-dependency data persistence.

The currently executable native surface is `sql.open(String)`,
`sql.execute(Int, String)`, and `sql.close(Int)`, each returning `Result(Int, Int)`.
Close returns `Ok(0)`, invalid or closed handles return `Err(3)`, and the 256-live-
connection quota returns `Err(4)` before an additional open. Closed IDs are never
reused. [Connection lifecycle](docs/SQL_LIFECYCLE.md) documents cleanup and
transaction semantics. The remote, parameterized, query, and migration examples
below describe the broader intended database design.

### Opening a Database

```
import db.sql

fn main() -> Result((), Error):
    # Embedded mode (local file)
    let db = sql.open("app.db")?
    
    # Or connect to remote libSQL server (Turso)
    let db = sql.connect("libsql://production.turso.io")?
    
    # Same API for both!
```

### Basic Operations

```
# Execute DDL
sql.execute(db, """
    CREATE TABLE IF NOT EXISTS users (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        email TEXT UNIQUE NOT NULL,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    )
""", [])?

# Insert data
sql.execute(db, 
    "INSERT INTO users (name, email) VALUES (?, ?)",
    [name, email]
)?

# Query data
let rows = sql.query(db,
    "SELECT id, name, email FROM users WHERE id = ?",
    [user_id]
)?

# Parse results
match rows:
    [row] ->
        let user = User(
            id: row.get("id"),
            name: row.get("name"),
            email: row.get("email")
        )
        Ok(user)
    [] -> Err(Error.not_found())
    _ -> Err(Error.multiple_results())
```

### Type-Safe Query Builder (Future)

```
# Type-safe queries (planned for future version)
let users = db
    |> select(User)
    |> where((u) -> u.age > 18)
    |> order_by((u) -> u.name)
    |> limit(10)
    |> fetch_all()?

# Generates SQL:
# SELECT id, name, email, age FROM users 
# WHERE age > 18 
# ORDER BY name 
# LIMIT 10
```

### Migrations

```
import db.migrate

fn main() -> Result((), Error):
    let db = sql.open("app.db")?
    
    # Run migrations from directory
    migrate.run(db, "./migrations")?
    
    # Migrations are tracked in schema_migrations table
    Ok(())
```

Migration files:
```
# migrations/001_create_users.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT UNIQUE NOT NULL
);

# migrations/002_add_timestamps.sql
ALTER TABLE users ADD COLUMN created_at DATETIME;
ALTER TABLE users ADD COLUMN updated_at DATETIME;
```

### Transactions

```
fn transfer(db: Database, from: UserId, to: UserId, amount: Int) -> Result((), Error):
    sql.transaction(db, fn(tx) ->
        # Deduct from sender
        sql.execute(tx, 
            "UPDATE accounts SET balance = balance - ? WHERE user_id = ?",
            [amount, from]
        )?
        
        # Add to receiver
        sql.execute(tx,
            "UPDATE accounts SET balance = balance + ? WHERE user_id = ?",
            [amount, to]
        )?
        
        Ok(())
    )
    # Automatically commits on Ok, rolls back on Err
```

### Complete Example: REST API with Database

```
module main

import http
import db.sql
import json

type User derive(Show, Eq):
    id: Int
    name: String
    email: String

fn main() -> Result((), Error):
    # Initialize database
    let db = sql.open("api.db")?
    init_db(db)?
    
    # Start server
    http.serve(8080, handle(db))

fn init_db(db: Database) -> Result((), Error):
    sql.execute(db, """
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            email TEXT UNIQUE NOT NULL
        )
    """, [])?
    Ok(())

fn handle(req: Request, db: Database) -> Response:
    match (req.method, req.path):
        (Get, "/users") ->
            match list_users(db):
                Ok(users) -> Response.json(users)
                Err(e) -> Response.error(500, e)
        
        (Get, "/users/{id}") ->
            match get_user(db, id.parse()?):
                Ok(user) -> Response.json(user)
                Err(_) -> Response.not_found()
        
        (Post, "/users") ->
            match req.body |> json.decode(User):
                Ok(user) ->
                    match create_user(db, user):
                        Ok(created) -> Response.created(created)
                        Err(e) -> Response.error(400, e)
                Err(e) -> Response.bad_request(e)
        
        (Delete, "/users/{id}") ->
            match delete_user(db, id.parse()?):
                Ok(_) -> Response.no_content()
                Err(_) -> Response.not_found()
        
        _ -> Response.not_found()

fn list_users(db: Database) -> Result(List(User), Error):
    sql.query(db, "SELECT id, name, email FROM users", [])?
        |> map(parse_user)
        |> collect()

fn get_user(db: Database, id: Int) -> Result(User, Error):
    let rows = sql.query(db,
        "SELECT id, name, email FROM users WHERE id = ?",
        [id]
    )?
    
    match rows:
        [row] -> parse_user(row)
        [] -> Err(Error.not_found())
        _ -> Err(Error.unexpected())

fn create_user(db: Database, user: User) -> Result(User, Error):
    sql.execute(db,
        "INSERT INTO users (name, email) VALUES (?, ?)",
        [user.name, user.email]
    )?
    
    let id = sql.last_insert_id(db)
    get_user(db, id)

fn delete_user(db: Database, id: Int) -> Result((), Error):
    sql.execute(db, "DELETE FROM users WHERE id = ?", [id])?
    Ok(())

fn parse_user(row: Row) -> Result(User, Error):
    Ok(User(
        id: row.get_int("id")?,
        name: row.get_string("name")?,
        email: row.get_string("email")?
    ))
```

### Scaling Path

**Stage 1: Embedded (MVP)**
```
# Single binary with embedded SQLite
let db = sql.open("app.db")
# Perfect for: MVPs, prototypes, small apps
# Handles: Millions of rows, GBs of data
```

**Stage 2: Replicated (Growing)**
```
# Multiple instances with read replicas
let primary = sql.open("primary.db")
let replica = sql.replicate_from("libsql://primary.turso.io")

# Writes to primary, reads from replica
```

**Stage 3: Managed (Scale)**
```
# Fully managed libSQL (Turso)
let db = sql.connect("libsql://production.turso.io")

# Same code, just change one line!
# Get: automatic backups, replication, edge deployment
```

### Why libSQL vs Plain SQLite?

**libSQL advantages:**
1. **Embedded + Remote modes** (smooth scaling path)
2. **Built-in replication** (multi-region support)
3. **Extensions** (vector search, FTS, JSON functions)
4. **WebAssembly support** (future: run Fern in browser)
5. **Commercial backing** (Turso provides managed hosting)
6. **SQLite compatible** (can use existing SQLite tools)

### Binary Size Impact

- **libSQL**: ~2 MB (largest component of server builds)
- **Alternative**: Use `--no-db` flag to exclude (saves 2 MB)
- **Worth it**: Replaces PostgreSQL client libs anyway (~1 MB)

---

## Example Program

```
module main

import http.server
import json

type Todo:
    id: Int
    title: String
    completed: Bool

fn main() -> Result((), Error):
    let todos = []
    
    server.start(8080, fn(req) -> handle_request(req, todos))

fn handle_request(req: Request, todos: List(Todo)) -> Response:
    match (req.method, req.path):
        (Get, "/todos") ->
            todos
                |> json.encode()
                |> Response.ok()
        
        (Post, "/todos") ->
            let todo = req.body |> json.decode(Todo)?
            let new_todos = todos |> append(todo)
            Response.created(json.encode(todo))
        
        (Delete, "/todos/{id}") ->
            let filtered = todos |> filter((t) -> t.id != id)
            Response.no_content()
        
        _ -> Response.not_found()

fn filter_active(todos: List(Todo)) -> List(Todo):
    [todo for todo in todos if not todo.completed]
```

---

## Compilation

```bash
# Compile to binary
fern build main.fn -o myapp

# Run directly
fern run main.fn

# Check types without compiling
fern check main.fn

# Format code
fern fmt src/

# Start REPL
fern repl
```

Output is a native executable that runs without the Fern compiler. Ordinary host
builds retain platform system-library dependencies. The Linux musl web packaging
path separately verifies a fully static executable with embedded browser assets.

---

## Scripting

Fern supports shebang scripts for quick scripting without explicit compilation:

```fern
#!/usr/bin/env fern run
fn main():
    let name = "World"
    println("Hello, {name}!")
```

Make the file executable and run it directly:

```bash
chmod +x script.fn
./script.fn
```

This is similar to `uvx` for Python or `deno run` for TypeScript. The `fern run` command compiles and executes in one step, cleaning up temporary files automatically.

**Use cases:**
- Quick automation scripts
- Build tools and dev utilities
- Data processing pipelines
- CLI tools during development

**String interpolation** makes scripting convenient:

```fern
#!/usr/bin/env fern run
fn main():
    let count = 42
    let status = "complete"
    println("Processed {count} items, status: {status}")
```

---

## Built-in Modules

Fern provides built-in modules for common operations. These are always available without imports and use a `Module.function()` syntax for clarity and discoverability.

### String Module

Functions for string manipulation:

```fern
# Length and basic operations
let len = String.len("hello")              # 5
let upper = String.to_upper("hello")       # "HELLO"
let lower = String.to_lower("HELLO")       # "hello"

# Concatenation
let full = String.concat("Hello", " World") # "Hello World"

# Comparison
let eq = String.eq("a", "a")               # true
let starts = String.starts_with("hello.txt", "hello")  # true
let ends = String.ends_with("hello.txt", ".txt")       # true
let has = String.contains("hello world", "world")      # true

# Slicing and trimming
let sub = String.slice("hello", 0, 3)      # "hel"
let trimmed = String.trim("  hello  ")     # "hello"
let left = String.trim_start("  hello")    # "hello"
let right = String.trim_end("hello  ")     # "hello"

# Transformation
let replaced = String.replace("hello", "l", "L")  # "heLLo"
let repeated = String.repeat("-", 10)      # "----------"

# Predicates
let empty = String.is_empty("")            # true
let digits = String.is_decimal("١２3")     # true: Unicode 16 decimal digits
let superscript = String.is_decimal("²")  # false: numeric, but not decimal

# Splitting and joining
let parts = String.split("a,b,c", ",")     # ["a", "b", "c"]
let lines = String.lines("a\nb\nc")        # ["a", "b", "c"]
let joined = String.join(["a", "b"], "-")  # "a-b"

# Character access
let idx = String.index_of("hello", "ll")   # Some(2)
let ch = String.char_at("hello", 0)        # Some(104) - ASCII 'h'
```

`String.is_decimal(String) -> Bool` is true exactly for nonempty text containing
only Unicode 16.0.0 General_Category=Nd scalars. It accepts mixed digit scripts;
signs, separators and other numeric characters are false. The native content
ceiling is 16 MiB; oversize raises the existing controlled String-size fault.
This predicate does not change numeric literal or conversion syntax. See
[the pinned text classification contract](docs/STRING_DECIMAL.md).

### List Module

Functions for list operations:

```fern
# Length and access
let len = List.len([1, 2, 3])              # 3
let first = List.get([1, 2, 3], 0)         # 1
let same = [1, 2, 3][0]                  # Bracket syntax uses List.get semantics
let member = 2 in [1, 2, 3]             # Scalar membership, equivalent result to List.contains
let head = List.head([1, 2, 3])            # 1
let tail = List.tail([1, 2, 3])            # [2, 3]

# Modification (returns new list)
let extended = List.push([1, 2], 3)        # [1, 2, 3]
let reversed = List.reverse([1, 2, 3])     # [3, 2, 1]
let combined = List.concat([1, 2], [3, 4]) # [1, 2, 3, 4]

# Predicates
let empty = List.is_empty([])              # true
```

### File Module

Functions for file I/O operations (return `Result` types):

```fern
# Reading and writing
let result = File.read("data.txt")         # Result(String, Int)
let written = File.write("out.txt", "hi")  # Result(Int, Int)
let appended = File.append("log.txt", "x") # Result(Int, Int)

# File info
let exists = File.exists("data.txt")       # Bool
let size = File.size("data.txt")           # Result(Int, Int)
let deleted = File.delete("temp.txt")      # Result(Int, Int)

# Directory operations
let is_dir = File.is_dir("src")            # Bool
let entries = File.list_dir("src")         # Result(List(String), Int); handle errors
```

### System Module

Functions for system-level operations like command-line arguments, process control,
shell command execution, and environment variables:

```fern
# Command-line arguments
let count = System.args_count()            # Number of arguments (including program name)
let program = System.arg(0)                # Program name (first argument)
let first_arg = System.arg(1)              # First user argument
let all_args = System.args()               # List(String) of all arguments

# Process control
System.exit(0)                             # Exit with success code
System.exit(1)                             # Exit with error code

# Shell command execution
let result = System.exec("ls -la")         # Returns (Int, String, String)
let exit_code = result.0                   # Exit code (0 = success)
let stdout = result.1                      # Standard output as string
let stderr = result.2                      # Standard error as string

let result2 = System.exec_args(["ls", "-la"])  # Execute with argument list

# Environment variables
let home = System.getenv("HOME")           # Get environment variable (empty string if not set)
let ok = System.setenv("KEY", "value")     # Set environment variable (returns 0 on success)
```

Example CLI tool:

```fern
fn main():
    let count = System.args_count()
    
    if count < 2:
        println("Usage: program <name>")
        let _ = System.exit(1)
        1
    else:
        let name = System.arg(1)
        println("Hello, {name}!")
        0
```

Example shell script:

```fern
fn main():
    # Run a command and check its output
    let result = System.exec("git status --porcelain")
    
    if result.0 != 0:
        println("Error: {result.2}")
        System.exit(1)
    
    if String.len(result.1) == 0:
        println("Working directory is clean")
    else:
        println("Uncommitted changes:")
        println(result.1)
    
    0
```

### Regex Module

Functions for regular expression pattern matching and text manipulation:

```fern
# Pattern matching
let matches = Regex.is_match("hello world", "world")   # Bool - true if pattern found
let match = Regex.find("hello world", "w\\w+")         # First match or null
let all = Regex.find_all("a1b2c3", "\\d")              # List(String) of all matches

# Text replacement
let new = Regex.replace("hello world", "world", "fern")      # Replace first match
let all_new = Regex.replace_all("a1b2c3", "\\d", "X")        # Replace all matches

# Splitting
let parts = Regex.split("a,b;c", "[,;]")               # List(String) - ["a", "b", "c"]

# Capture groups
let caps = Regex.captures("abc123", "(\\w+)(\\d+)")    # Access groups with caps.0, caps.1, ...
```

Example - validating input:

```fern
fn is_valid_email(email: String) -> Bool:
    Regex.is_match(email, "^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$")

fn main():
    let email = System.arg(1)
    if is_valid_email(email):
        println("Valid email")
        0
    else:
        println("Invalid email")
        1
```

### Tui Module (Terminal UI)

Fern provides a `Tui.*` namespace for terminal user interface components, inspired by Python's Rich library. All modules are nested under `Tui.` for clear organization.

#### Tui.Term - Terminal Capabilities

Query terminal properties and capabilities:

```fern
# Get terminal size (columns, rows)
let size = Tui.Term.size()
let cols = size.0
let rows = size.1
println("Terminal is {cols}x{rows}")

# Check if running in a TTY (interactive terminal)
if Tui.Term.is_tty():
    println("Running interactively")
else:
    println("Running in a pipe or script")

# Check color support level
# Returns: 0 = no color, 1 = basic (8/16), 2 = 256 colors, 3 = truecolor
let colors = Tui.Term.color_support()
match colors:
    0 -> println("No color support")
    1 -> println("Basic colors (8/16)")
    2 -> println("256 colors")
    3 -> println("Truecolor (24-bit)")
    _ -> println("Unknown")
```

#### Tui.Style - Text Styling

Apply ANSI colors and styles to terminal text:

```fern
# Basic colors (foreground)
println(Tui.Style.red("Error!"))
println(Tui.Style.green("Success!"))
println(Tui.Style.blue("Info"))
println(Tui.Style.yellow("Warning"))
println(Tui.Style.cyan("Cyan text"))
println(Tui.Style.magenta("Magenta text"))

# Bright colors
println(Tui.Style.bright_red("Bright red"))
println(Tui.Style.bright_green("Bright green"))

# Background colors
println(Tui.Style.on_red("Red background"))
println(Tui.Style.on_blue("Blue background"))

# Text attributes
println(Tui.Style.bold("Bold text"))
println(Tui.Style.dim("Dim text"))
println(Tui.Style.italic("Italic text"))
println(Tui.Style.underline("Underlined"))
println(Tui.Style.strikethrough("Strikethrough"))

# 256-color and RGB (for capable terminals)
println(Tui.Style.color("Text", 208))              # 256-color code
println(Tui.Style.rgb("Text", 255, 128, 0))        # RGB values
println(Tui.Style.hex("Text", "#FF8000"))          # Hex color

# Composable - nest for multiple styles
println(Tui.Style.bold(Tui.Style.red("Bold red")))
```

#### Tui.Panel - Bordered Panels

Create Rich-style bordered panels for content:

```fern
# Basic panel
let panel = Tui.Panel.new("Hello, World!")
println(Tui.Panel.render(panel))

# Panel with title and subtitle
let panel = Tui.Panel.new("Content goes here")
let panel = Tui.Panel.title(panel, "My Panel")
let panel = Tui.Panel.subtitle(panel, "Subtitle text")
println(Tui.Panel.render(panel))

# Border styles: "rounded", "heavy", "double", "ascii"
let panel = Tui.Panel.border(panel, "rounded")

# Colored borders
let panel = Tui.Panel.border_color(panel, "cyan")   # "red", "green", "blue", "yellow", "cyan", "magenta"

# Width and padding
let panel = Tui.Panel.width(panel, 60)
let panel = Tui.Panel.padding(panel, 2)

println(Tui.Panel.render(panel))
```

#### Tui.Table - Data Tables

Create formatted tables with headers and rows:

```fern
# Create table with columns
let table = Tui.Table.new()
let table = Tui.Table.title(table, "Users")
let table = Tui.Table.add_column(table, "Name")
let table = Tui.Table.add_column(table, "Age")
let table = Tui.Table.add_column(table, "City")

# Add rows (as lists)
let row1 = ["Alice", "30", "NYC"]
let table = Tui.Table.add_row(table, row1)
let row2 = ["Bob", "25", "LA"]
let table = Tui.Table.add_row(table, row2)

# Border styles
let table = Tui.Table.border(table, "rounded")

# Hide header row if needed
let table = Tui.Table.show_header(table, 0)  # 0 = hide, 1 = show

println(Tui.Table.render(table))
```

#### Tui.Status - Status Badges

Display colored status badges for logging:

```fern
println(Tui.Status.ok("All systems operational"))
println(Tui.Status.warn("Low disk space"))
println(Tui.Status.error("Connection failed"))
println(Tui.Status.info("Press Ctrl+C to cancel"))
println(Tui.Status.debug("Verbose logging enabled"))
```

Output:
```
 OKAY  All systems operational
 WARN  Low disk space
 FAIL  Connection failed
 INFO  Press Ctrl+C to cancel
 DBUG  Verbose logging enabled
```

#### Tui.Live - Same-Line Updates

Update text on the same line (for spinners, progress):

```fern
# Print without newline (stays on same line)
Tui.Live.print("Loading... ")

# Update the current line
Tui.Live.update("Loading... 50%")
Tui.Live.sleep(500)
Tui.Live.update("Loading... 100%")

# Clear the line
Tui.Live.clear_line()

# Finish with newline
Tui.Live.done()
```

#### Tui.Progress - Progress Bars

Display progress bars with percentage:

```fern
let progress = Tui.Progress.new(100)  # Total items
let progress = Tui.Progress.description(progress, "Downloading")
let progress = Tui.Progress.width(progress, 40)

# Update progress
for i in 0..100:
    let progress = Tui.Progress.set(progress, i)
    Tui.Live.update(Tui.Progress.render(progress))
    Tui.Live.sleep(50)

Tui.Live.done()
```

#### Tui.Spinner - Animated Spinners

Display animated spinners for long-running tasks:

```fern
let spinner = Tui.Spinner.new()
let spinner = Tui.Spinner.message(spinner, "Processing")
let spinner = Tui.Spinner.style(spinner, "dots")  # "dots", "line", "braille"

# Animate in a loop
for i in 0..20:
    let spinner = Tui.Spinner.tick(spinner)
    Tui.Live.update(Tui.Spinner.render(spinner))
    Tui.Live.sleep(100)

Tui.Live.done()
```

#### Tui.Prompt - Interactive Input

```fern
let name = Tui.Prompt.input("Your name: ")
let secret = Tui.Prompt.password("Password: ")
let confirmed = Tui.Prompt.confirm("Continue?")
let choice = Tui.Prompt.select("Choose a target", ["native", "wasm"])
let count = Tui.Prompt.int("Workers", 1, 16)
```

`input` and `password` use the bundled line editor when both stdin and stdout
are terminals: left/right arrows, Home/End, Ctrl+A/Ctrl+E, Backspace, Delete,
Ctrl+U/Ctrl+K, and UTF-8 character editing are supported. Password characters
are masked; terminal settings are restored on Enter, Ctrl+C, and Ctrl+D.
EOF or cancellation returns an empty string. Redirected input reads one plain
line without terminal escapes. Interactive lines use the editor's 4096-byte buffer.
`confirm`, `select`, and `int` retain their line-oriented input behavior.

#### Tui.Tree - Immutable Hierarchies

```fern
let root = Tui.Tree.new("project")
let source = Tui.Tree.add(Tui.Tree.new("src"), Tui.Tree.new("main.fn"))
let tree = Tui.Tree.add(Tui.Tree.add(root, source), Tui.Tree.new("README.md"))
println(Tui.Tree.render(tree))
```

```text
project
├── src
│   └── main.fn
└── README.md
```

`add(parent, child)` appends a whole subtree and returns a new `Tree`, preserving
both inputs. `render` returns plain Unicode text without a trailing newline;
multiline labels retain their branch indentation. Tree composition works with
pre-rendered child text, avoiding recursive traversal and a fixed nesting limit.

#### Tui.Log - Deterministic Log Records

```fern
println(Tui.Log.debug("Build details"))
println(Tui.Log.info("Build started"))
println(Tui.Log.warn("No tests found"))
println(Tui.Log.error("Build failed"))
```

Each function returns a plain string such as `[INFO] Build started`, with no
implicit output, timestamp, color, or trailing newline. Newlines, carriage
returns, tabs, backslashes, and ASCII control characters in messages are escaped,
so each record occupies one line and terminal escape sequences remain visible text.

#### Additional Terminal Cursor Controls

```fern
Tui.Term.save_cursor()
Tui.Term.move_to(2, 5)  # One-based row and column, each clamped to at least 1
Tui.Term.up(1)
Tui.Term.down(1)
Tui.Term.left(2)
Tui.Term.right(2)
Tui.Term.hide_cursor()
Tui.Term.show_cursor()
Tui.Term.restore_cursor()
Tui.Term.clear()       # Clear screen and move to top left
```

These operations return Unit and write ANSI sequences only when stdout is a
terminal. Relative movement by zero or a negative count does nothing. Pair cursor
hiding with showing before returning from your application.

### I/O Functions

Basic print functions (always available):

```fern
print(42)              # Print without newline
println("Hello")       # Print with newline

# Polymorphic - accepts Int, String, Bool
println(42)            # "42"
println("hi")          # "hi"
println(true)          # "true"
```

---

## Design Decisions

### Type System ✅ Decided

1. **Generics syntax**: `List(a)` with lowercase type variables
2. **Traits**: Rust/Gleam-style with `trait`/`impl` keywords
3. **Union types**: Set-theoretic unions (Elixir-inspired): `String | Int`
4. **Newtypes**: Distinct compile-time types with zero runtime cost
5. **No null**: Only `Option(a)` for optional values
6. **Trait derivation**: Automatic via `derive(Show, Eq, Ord, Clone)`
7. **Type inference**: Required at public API boundaries, optional internally
8. **Variance**: Invariant by default (keep simple for v1)

### Concurrency Model ✅ Decided

1. **Actor-based**: Erlang/Elixir-style lightweight processes
2. **Type-safe messages**: `Pid(msg)` ensures type safety
3. **Immutable messages**: All message passing is immutable
4. **Supervision**: Processes can monitor and restart each other
5. **Runtime overhead**: ~300-400 KB for actor scheduler
6. **Compilation modes**: Auto-detect (actors used = include runtime)
7. **Standard library**: Pre-built actors for cache, queue, pubsub patterns

### Database Strategy ✅ Decided

1. **Embedded libSQL**: SQLite-compatible, ~2 MB overhead
2. **Dual modes**: Embedded file OR remote connection (same API)
3. **Scaling path**: Start embedded, scale to Turso managed hosting
4. **Optional inclusion**: Use `--no-db` flag to exclude (CLI tools)
5. **Migrations**: Built-in migration runner
6. **Transactions**: Functional API with automatic rollback on error

### Dual-Purpose Philosophy ✅ Decided

1. **CLI mode**: < 1 MB binaries, < 5ms startup, no runtime
2. **Server mode**: 2-4 MB binaries, full actor runtime + libSQL
3. **Auto-detection**: Compiler chooses mode based on code
4. **Same language**: No separate syntax or APIs for each mode
5. **Performance targets**: Competitive with Go/Rust for both modes

### Implementation Strategy ✅ Decided

1. **Compiler language**: Rust Cargo workspace
2. **Backend**: Cranelift native object generation
3. **Memory**: Rust-owned nonmoving tracing with actor payload heaps and explicit roots; conservative native scanning remains
4. **Type safety**: Checked semantic IR and validated native ABI/lowering boundaries
5. **Data structures**: Rust-owned compiler collections and bounded native value layouts
6. **Browser**: Separate scalar/String WASM emitter and Rust-authored browser host
7. **Development**: Test-first changes, independent native/WASM execution oracles, formatting, Clippy and bounded fuzz checks

### Standard Library ✅ Decided

1. **Two-tier structure**: Core (always) + Standard Library (tree-shaken)
2. **Core modules**: list, map, set, option, result, string, int, float, bool
3. **45+ stdlib modules**: I/O, data formats, HTTP, database, concurrency, testing, CLI utilities
4. **Tree-shaking**: Import only what you use, pay only for what you include
5. **Testing framework**: Built-in (test.assert, test.mock, test.bench)
6. **Size range**: 200 KB (core only) to 4-6 MB (everything)
7. **Batteries-included feel**: Common tasks easy without external dependencies

### FFI (Foreign Function Interface) ✅ Decided

1. **Foreign declarations**: `foreign "C" fn name(params) -> return_type`
2. **Type mapping**: Exact C-width scalar types and sealed `Ptr(a)` handles; see [FFI](docs/FFI.md)
3. **Safety**: Stdlib wraps unsafe C code in safe Fern APIs
4. **Users never see pointers**: Only stdlib authors use FFI
5. **Linking**: Automatic linking with specified C libraries

### Conditionals & Pattern Matching ✅ Decided

1. **if/else**: For simple true/false conditions (1-2 branches)
2. **match (no value)**: For complex condition chains (3+ branches)
3. **match (with value)**: For pattern matching and destructuring
4. **Unified approach**: Same `match` keyword for all pattern matching

### Function Style ✅ Decided

1. **Prefer multiple clauses**: Elixir-style function clauses are idiomatic
2. **Co-location enforced**: Compiler error if clauses are separated
3. **Use match when**: Need parameter name, complex multi-arg patterns, or guards
4. **Exhaustiveness checking**: Compiler warns on missing patterns

### Mutation ✅ Decided

1. **No `mut` keyword**: Language is immutable by default
2. **Rebinding allowed**: `let x = x + 1` creates new binding (shadowing)
3. **No `while` or `loop`**: Use recursion or functional combinators (Gleam-style)
4. **`for` iteration**: Iterate over collections (no mutation needed)
5. **Performance escape hatch**: Stdlib can use C FFI for critical code
6. **v1 decision**: Wait for real performance data before considering mutation

### Error Handling & Panics ✅ Decided

1. **No panic mechanism**: Fern has no `panic()` function or `.unwrap()` method
2. **All errors via Result**: Every fallible operation returns `Result(ok, err)`
3. **Compiler enforcement**: Unhandled Result values cause compile errors
4. **Programs never crash**: All errors are recoverable
5. **No exceptions**: No try/catch mechanism
6. **`?` operator**: Propagate Result or early return (Rust-style, postfix)
7. **`with` expression**: Complex error handling with `<-` bindings and pattern matching
8. **`defer` statement**: Guaranteed cleanup (non-actor functions only)
9. **Debug assertions**: `debug_assert()` for development only (removed in release)

**Rationale:**
- Predictability: Programs never crash unexpectedly from errors
- Actor safety: Actor crashes would lose messages and state
- Server reliability: Servers log errors and continue serving
- Better debugging: Errors are logged, not crashes
- Simpler mental model: Only one error handling path (Result)
- `?` is familiar from Rust and shows WHAT can fail (postfix on the call)
- `with`/`<-` for complex cases where different errors need different handling
- Learned from Rust: Panics cause problems in production code

### REPL ✅ Decided

**Decision**: Include REPL in v0.5+ (after core language stabilizes)

**Rationale:**
- Important for exploration and learning
- Requires tree-walking interpreter (separate from compiler)
- Not critical for v0.1-0.3 (focus on compilation first)

**Implementation:**
- Tree-walking interpreter in the CLI
- Full type checking
- Can evaluate expressions, define functions interactively
- Load modules and inspect types

### Comptime ✅ Decided

**Decision**: Include comptime (Zig-style compile-time execution) instead of macros

**Rationale:**
- More powerful than traditional macros
- Safer than text-based macro systems
- Enables DSL building (SQL, HTML, regex)
- Runs real Fern code at compile time

**Use cases:**
```fern
# Compile-time constants
const pi = comptime:
    355.0 / 113.0  # Computed at compile time

# Compile-time code generation
const sql_schema = comptime:
    generate_schema(User)  # Generates CREATE TABLE at compile time

# Compile-time validation
const validated_regex = comptime:
    regex.compile("[a-z]+") or compile_error("invalid regex")
```

**Implemented now**: bounded, pure `const name = comptime:` evaluation with
ordinary Fern arithmetic, collections, nominal values and closures. See
[compile-time constants](docs/COMPTIME.md) for exact limits and execution tests.
Type reflection/code-generation helpers and compile-time native resources in the
illustrative examples above remain design proposals.

### Tooling ✅ Decided

**Built into `fern` CLI:**

1. **Compiler** (v0.1)
   - `fern build` - Compile to binary
   - `fern run` - Compile and run
   - `fern check` - Type check only

2. **Formatter** (v0.2)
   - `fern fmt` - Format code (automatic indentation, consistency)
   - Rust frontend: `fern-rs fmt <source.fn|directory>` validates all selected sources before staging replacements. `fern-rs fmt --check <source.fn|directory>` checks canonical formatting without writing; exit0 means clean, exit1 means drift or an input error. Directory discovery skips hidden/build/dependency directories and child symlinks (Decision99).
   - Style guide enforced automatically

3. **LSP** (v0.3)
   - `fern lsp` - Language server for editors
   - Provides autocomplete, go-to-definition, hover info, etc.
   - Works with VSCode, Zed, Neovim, etc.

4. **Linter** (v0.3)
   - `fern lint` - Static analysis
   - Catches common mistakes, suggests improvements
   - Integrated into LSP

5. **Doc Generator** (v0.4)
   - `fern doc` - Generate documentation from `@moduledoc`/`@doc` comments
   - Outputs Markdown, one standalone HTML page, or a multi-page site
     (`--site`) with guides, sidebar navigation, local search and
     cross-references, like HexDocs
   - Examples extracted and tested

6. **REPL** (v0.5)
   - `fern repl` - Interactive environment
   - Explore APIs, test expressions

**Rationale:**
- Users expect LSP and tooling built-in (modern language requirement)
- Reduces ecosystem fragmentation
- Ensures consistent experience across editors
- AI development benefits from integrated tooling

### Compiler Error Messages ✅ Decided

Fern prioritizes **helpful, friendly error messages** that guide users toward solutions.

**Design Philosophy:**

1. **Be specific** - Tell exactly what went wrong and where
2. **Be helpful** - Suggest how to fix it
3. **Be friendly** - No jargon, explain concepts when needed
4. **Show context** - Display relevant code with highlighting
5. **Actionable** - Provide concrete next steps

**Error Message Structure:**

```
Error: [What went wrong]
  ┌─ [file path]:[line]:[column]
  │
[line] │ [code with problem]
       │ [visual indicator pointing to issue]
  │
  = [Explanation in plain language]
  = help: [Concrete suggestion to fix]
  = note: [Additional context if helpful]
```

**Examples:**

**1. Unhandled Result:**
```
Error: Result value must be handled
  ┌─ src/main.fn:5:17
  │
5 │     let data = read_file("config.txt")
  │                ^^^^^^^^^^^^^^^^^^^^^^^ this returns Result(String, IoError)
  │
  = Result values represent operations that can fail
  = help: Handle the error using one of these approaches:
      1. Propagate with ?: let data = read_file("config.txt")?
      2. Match on Result: match read_file(...): Ok(d) -> ..., Err(e) -> ...
      3. Use combinator: read_file(...) |> unwrap_or(default)
  = note: See error handling guide: https://fern-lang.org/errors
```

**2. Type Mismatch:**
```
Error: Type mismatch
  ┌─ src/math.fn:12:12
   │
12 │     return "not a number"
   │            ^^^^^^^^^^^^^^ expected Int, found String
   │
   = This function has return type Int
   = help: Return an integer value, for example:
       return 0
   = help: Or if you want to return an error, change return type:
       fn divide(...) -> Result(Int, String):
```

**3. Missing Pattern:**
```
Error: Non-exhaustive patterns
  ┌─ src/handler.fn:8:5
  │
8 │     match status:
  │     ^^^^^^^^^^^^^ missing pattern: Pending(_)
  │
  = The type Status has these variants:
      - Active
      - Inactive
      - Pending(DateTime)
  = help: Add a pattern to handle Pending:
      match status:
          Active -> ...
          Inactive -> ...
          Pending(since) -> ...  # Add this
```

**4. Unused Variable:**
```
Warning: Unused variable
  ┌─ src/process.fn:15:9
   │
15 │     let result = compute()
   │         ^^^^^^ this variable is never used
   │
   = help: If intentionally unused, prefix with underscore:
       let _result = compute()
   = help: Or use the wildcard pattern:
       let _ = compute()
   = note: Remove this binding if not needed
```

**5. Function Clause Separation:**
```
Error: Function clauses must be adjacent
  ┌─ src/math.fn:8:1
  │
3 │ fn factorial(0) -> 1
  │    --------- first clause defined here
  │
6 │ fn other_function() -> 42
  │    -------------- intervening function
  │
8 │ fn factorial(n) -> n * factorial(n - 1)
  │    ^^^^^^^^^ clause must be adjacent to first clause
  │
  = Fern requires all clauses of a function to be grouped together
  = help: Move this clause next to line 3, or rename the function
```

**6. Circular Dependency:**
```
Error: Circular module dependency detected
  ┌─ src/a.fn:3:8
  │
3 │ import b
  │        ^ importing b
  │
  ┌─ src/b.fn:3:8
  │
3 │ import a
  │        ^ which imports a
  │
  = Dependency cycle: a → b → a
  = help: Extract shared code into a new module:
      1. Create src/shared.fn with common types
      2. Have both a.fn and b.fn import shared
  = note: See module organization guide: https://fern-lang.org/modules
```

**7. Null Pointer (FFI Boundary):**
```
Error: Potential null pointer
  ┌─ stdlib/db/sql.fn:42:5
   │
42 │     Ok(Database(db_ptr))
   │        ^^^^^^^^^^^^^^^^^ db_ptr might be null
   │
   = C functions can return NULL pointers
   = help: Check for null before wrapping:
       return Err(SqlError.null_pointer()) if db_ptr == null
       Ok(Database(db_ptr))
   = note: This is in FFI layer - only stdlib authors see this
```

**8. Actor Type Mismatch:**
```
Error: Type mismatch in message send
  ┌─ src/cache.fn:24:10
   │
24 │     send(cache, "invalid")
   │                 ^^^^^^^^^ expected CacheMsg(String, User), found String
   │
   = The process cache has type Pid(CacheMsg(String, User))
   = It only accepts these message types:
      - Get(String, Pid(Option(User)))
      - Set(String, User)
      - Delete(String)
   = help: Send a valid CacheMsg variant:
       send(cache, Set("user:123", user))
```

**Error Categories:**

- **Errors** (must fix) - Type errors, unhandled Results, syntax errors
- **Warnings** (should fix) - Unused variables, missing docs, style violations
- **Notes** (informational) - Suggestions for improvement, links to docs

**Error Recovery:**

- Compiler attempts to recover and show multiple errors when possible
- Limits to first 10 errors to avoid overwhelming output
- Groups related errors together

**Interactive Mode:**

```bash
# Show full error with examples
fern check --explain E0001

# JSON output for editor integration
fern check --format=json

# Show only errors (no warnings)
fern check --errors-only
```

**Error Codes:**

All errors have codes for documentation lookup:
- `E0001` - Type mismatch
- `E0002` - Unhandled Result
- `E0003` - Non-exhaustive patterns
- `E0004` - Unused binding
- And so on...

**Documentation:**

Each error code links to detailed documentation:
```
= note: For more information, see error code E0002
        https://fern-lang.org/errors/E0002
```

**Comparison with Other Languages:**

```
# Fern ✅ Friendly and actionable
Error: Result value must be handled
  = help: Propagate with ?: let data = read_file(...)?

# Typical C/C++ ❌ Cryptic
error: invalid conversion from 'const char*' to 'int'

# Fern ✅ Shows context
  5 │     let data = read_file("config.txt")
    │                ^^^^^^^^^^^^^^^^^^^^^^^ returns Result

# Typical Java ❌ Stack trace
Exception in thread "main" java.lang.NullPointerException
    at Main.process(Main.java:42)
```

**Goal:** Make compiler errors feel like a helpful pair programmer, not a gatekeeeper.

### Labeled Arguments ✅ Decided

**Decision**: Support labeled arguments for clarity

**Syntax:**
```fern
fn connect(host: String, port: Int, timeout: Int):
    # ...

# Call with labels (any order)
connect(host: "localhost", port: 8080, timeout: 5000)

# The unique String parameter may remain positional
connect("localhost", port: 8080, timeout: 5000)
```

**Required labels:**
- Multiple parameters of same type (prevents confusion)
- All Bool parameters (ambiguous without labels)

**Rationale:**
- Clearer API calls (self-documenting)
- Flexible argument order with labels
- Common in modern languages (Swift, Kotlin, Python)
- Prevents mistakes with similar-typed parameters

### Doc Tests ✅ Decided

**Decision**: Examples in `@doc` comments are automatically tested

**Syntax:**
```fern
@doc """
# Examples

```fern
divide(10, 2)  # => Ok(5)
divide(10, 0)  # => Err(_)
```
"""
pub fn divide(a: Int, b: Int) -> Result(Int, DivideError):
    # ...
```

**Run with:** `fern test --doc` or `fern test` (includes doc tests)

**Rationale:**
- Examples always work (tested automatically)
- Documentation stays accurate
- Great for TDD and AI learning
- Catches breaking changes

### Package Management ✅ Decided

**Package file**: `fern.toml`

```toml
[package]
name = "myapp"
version = "0.1.0"
authors = ["Niklas Heer <niklas@example.com>"]

[dependencies]
http = "1.0.0"
json = "0.5.2"
```

**Package registry**: Central registry (like crates.io, npm)

**CLI commands:**
```bash
fern new myapp           # Create new project
fern add http            # Add dependency
fern install             # Install dependencies
fern publish             # Publish to registry
```

**Versioning**: Semantic versioning (semver)

**Resolution**: Cargo-style (lock file for reproducibility)

**Timeline**: v0.4+

### Open Questions

None! All major design decisions have been made. Ready to start implementation.

---

## FFI (Foreign Function Interface)

Native foreign declarations are implemented with explicit C scalar widths and
sealed `Ptr(a)` handles. See [the FFI contract](docs/FFI.md) for supported syntax,
checked conversions, ownership obligations, linker behavior and acceptance tests.

```fern
foreign "C" fn absolute(value: Int) -> Int as "llabs"
foreign "C" fn cosine(value: Float) -> Float as "cos" from "m"

fn main():
    println(absolute(-42))
    println(cosine(0.0))
```

The original raw-pointer sketch (`*mut`, `*const`, address-of, mutable output
parameters and arbitrary dereference) is superseded by this sealed boundary.
Those sketches are not supported syntax. Rust adapters translate complex C
interfaces to explicit scalars and typed handles. Foreign declarations are
trusted contracts, not a guarantee that arbitrary C code is memory safe.
Borrowed String owners survive returned interior pointers and collection;
foreign-owned allocation and destruction remain the wrapper author's contract.

### Performance Escape Hatch

Runtime internals may use mutation in Rust behind the checked native ABI. A
standard-library optimization must preserve Fern's immutable aliases, checked
errors and collector roots. It needs an independent output/ABI regression and
measurement showing the improvement. Fern-owned C implementations are retired;
third-party native libraries may still be accessed through Rust wrappers.

---

## Conditionals and Pattern Matching

Fern uses `if/else` for simple conditionals and `match` for pattern matching and complex conditions.

### if/else (Simple Conditionals)

For simple true/false branches:

```fern
# Basic if/else
let category = if age < 18:
    "minor"
else:
    "adult"

# No else (returns () if false)
if should_log:
    log("message")

# Chained (but prefer match for 3+ conditions)
let grade = if score >= 90:
    "A"
else if score >= 80:
    "B"
else:
    "C"
```

**Use if/else when:**
- 1-2 branches
- Simple true/false
- Obvious logic

### match without Value (Condition Chains)

For complex conditions (3+ branches):

```fern
# Better than long if/else-if chains
let category = match:
    age < 13 -> "child"
    age < 20 -> "teenager"
    age < 65 -> "adult"
    _ -> "senior"

# Complex boolean expressions
match:
    x > 10 && y > 10 -> "both large"
    x > 10 -> "x large"
    y > 10 -> "y large"
    _ -> "both small"
```

**Use match (no value) when:**
- 3+ conditions
- Complex boolean logic
- Order matters (first match wins)

### match with Value (Pattern Matching)

For destructuring and type matching:

```fern
# Pattern matching on types
match result:
    Ok(value) -> process(value)
    Err(error) -> handle_error(error)

# With guards
match value:
    x if x > 0 -> "positive"
    x if x < 0 -> "negative"
    _ -> "zero"

# Destructuring tuples
match point:
    (0, 0) -> "origin"
    (x, 0) -> "on x-axis"
    (0, y) -> "on y-axis"
    (x, y) -> "at ({x}, {y})"

# Multiple values
match (method, path):
    (Get, "/users") -> list_users()
    (Get, "/users/{id}") -> get_user(id)
    (Post, "/users") -> create_user()
    _ -> not_found()
```

**Use match (with value) when:**
- Pattern matching on types
- Destructuring needed
- Exhaustiveness checking desired
- Multiple values matched together

### Pattern Matching in let

Destructuring in bindings:

```fern
# Tuple destructuring
let (x, y) = get_point()

# List destructuring
let [first, second, ..rest] = items

# Type destructuring
let User(name, email, _) = current_user

# Option unwrapping with let-else
let Some(value) = optional else:
    return Err("no value")
```

---

## Function Definitions

Fern supports two styles: **multiple clauses** (preferred) and **single function with match**.

### Multiple Clauses (Preferred, Idiomatic)

Clauses use the same coverage rules as `match`: missing cases and unreachable
clauses are errors, and guards do not establish exhaustive coverage (Decision 63).
All clauses share arity, parameter types, visibility and supplied return annotations.
The current Rust checkpoint infers private parameter types when clause patterns
or supplied annotations provide complete evidence. Fully unconstrained private
signature generalization in some examples below remains migration work.

**The Fern way** - clean, readable, Elixir-inspired:

```fern
# Simple pattern matching
fn length([]) -> 0
fn length([_, ..rest]) -> 1 + length(rest)

# Recursion
fn factorial(0) -> 1
fn factorial(n) -> n * factorial(n - 1)

# Type-based dispatch
fn area(Circle(r)) -> pi() * r * r
fn area(Rectangle(w, h)) -> w * h
fn area(Triangle(b, h)) -> 0.5 * b * h

# Guards
fn classify(x) if x > 0 -> Positive
fn classify(x) if x < 0 -> Negative
fn classify(_) -> Zero
```

**Co-location enforcement:**

Clauses must be adjacent - compiler error if separated:

```fern
# ✅ OK: Clauses together
fn factorial(0) -> 1
fn factorial(n) -> n * factorial(n - 1)

fn fibonacci(0) -> 0
fn fibonacci(1) -> 1
fn fibonacci(n) -> fibonacci(n - 1) + fibonacci(n - 2)

# ❌ ERROR: Clauses separated
fn factorial(0) -> 1

fn other_function() -> 42  # Separation!

fn factorial(n) -> n * factorial(n - 1)  # Compiler error!
```

**Error message:**
```
Error: Function clauses must be adjacent
  ┌─ example.fn:5:1
  │
1 │ fn factorial(0) -> 1
  │    --------- first clause defined here
  │
4 │ fn other_function() -> 42
  │    -------------- intervening function
  │
5 │ fn factorial(n) -> n * factorial(n - 1)
  │    ^^^^^^^^^ clause must be adjacent to first clause
  │
  = help: move this clause next to line 1, or rename the function
```

### Single Function with match

**Use when:**

1. **Need parameter name:**
```fern
fn process(request: Request):
    log("Processing: {request.path}")
    match request.method:
        Get -> handle_get(request)
        Post -> handle_post(request)
        Delete -> handle_delete(request)
```

2. **Complex multi-argument patterns:**
```fern
fn handle(method: Method, path: String, body: String):
    match (method, path):
        (Get, "/users") -> list_users()
        (Get, "/users/{id}") -> get_user(id)
        (Post, "/users") -> create_user(body)
        (Delete, "/users/{id}") -> delete_user(id)
        _ -> not_found()
```

3. **Guards on multiple parameters:**
```fern
fn compare(x: Int, y: Int):
    match (x, y):
        (a, b) if a > b -> Greater
        (a, b) if a < b -> Less
        _ -> Equal
```

### Style Guide Summary

| Situation | Preferred Style | Example |
|-----------|----------------|---------|
| Simple patterns | Multiple clauses | `fn length([]) -> 0` |
| Type dispatch | Multiple clauses | `fn area(Circle(r)) -> ...` |
| Need param name | Single + match | `fn process(req): match req: ...` |
| Complex multi-arg | Single + match | `match (method, path): ...` |
| Guards on multiple | Single + match | `match (x, y): (a, b) if ...` |

---

## Immutability and Mutation

Fern is **immutable by default** with no `mut` keyword in v1.

### Rebinding (Shadowing)

Create new bindings with the same name:

```fern
let x = 1
let x = x + 1  # New binding, shadows old x
let x = x * 2  # Another new binding
print(x)       # 4
```

**Compiler optimizes this internally** - no performance penalty.

### Alternatives to Mutation

**1. Recursion with accumulators:**
```fern
fn sum_list(items: List(Int)) -> Int:
    sum_loop(items, 0)

fn sum_loop([], acc) -> acc
fn sum_loop([x, ..xs], acc) -> sum_loop(xs, acc + x)
```

**2. Fold/reduce (most idiomatic):**
```fern
fn sum_list(items: List(Int)) -> Int:
    items |> list.fold(0, (acc, x) -> acc + x)
```

### Performance Escape Hatch

Standard-library performance work belongs in Rust runtime helpers with explicit
ABI, ownership and error contracts. Internal mutation must preserve immutable
Fern semantics. Measure a real workload before adding a specialized helper;
third-party native code remains permitted through Rust wrappers.

### Why No `mut`, `while`, or `loop` in v1

**Reasons:**
- ✅ Keeps language simple and semantics clear
- ✅ Easier for AI to generate safe code
- ✅ Compiler can optimize rebinding and tail calls
- ✅ Recursion handles all stateful iteration patterns
- ✅ Matches functional-first philosophy (like Gleam)

**If mutation is truly needed later:**
- Collect real performance data
- Consider `unsafe { mut }` blocks
- Or keep using C FFI (cleaner boundary)

---

## Standard Library

Fern provides a comprehensive standard library with a **two-tier structure**: Core modules (always included) and Standard Library modules (tree-shaken based on imports).

### Design Philosophy

**"Batteries-included" without bloat:**
- Common tasks should be easy (HTTP, JSON, CSV, testing)
- CLI tools stay tiny (< 1 MB)
- Servers get everything they need
- Pay only for what you use (automatic tree-shaking)

### Two-Tier Structure

#### **Tier 1: Core (Always Included, ~200 KB)**

Language primitives that are always present:

```
core/
  ├─ list.fn           # List operations (map, filter, fold, etc.)
  ├─ map.fn            # Map/dictionary operations
  ├─ set.fn            # Set operations
  ├─ option.fn         # Option(a) type and operations
  ├─ result.fn         # Result(ok, err) type and operations
  ├─ string.fn         # String manipulation
  ├─ int.fn            # Integer operations
  ├─ float.fn          # Float operations
  └─ bool.fn           # Boolean operations
```

#### **Tier 2: Standard Library (Tree-Shaken)**

Import to include. These modules are automatically linked only if imported.

**I/O & System (~100-300 KB total):**
```
io/
  ├─ file.fn           # read, write, append, exists, delete
  ├─ path.fn           # join, dirname, basename, extension
  ├─ dir.fn            # list, create, remove, walk
  ├─ env.fn            # Environment variables, command-line args
  └─ process.fn        # Spawn processes, exec, pipes, exit codes

os.fn                  # Platform detection, home_dir, temp_dir
```

**Data Formats (~50-150 KB each):**
```
json.fn                # JSON parse/stringify, encoding/decoding
csv.fn                 # CSV read/write with headers
toml.fn                # TOML parsing (config files)
yaml.fn                # YAML parsing (common in DevOps)
xml.fn                 # XML parsing (APIs, feeds, legacy systems)
```

**Text Processing (~50-200 KB each):**
```
regex.fn               # Regular expressions
string/
  ├─ format.fn         # sprintf-style formatting
  ├─ template.fn       # Simple {{ var }} templating
  └─ unicode.fn        # Unicode normalization, grapheme clusters
```

**CLI Utilities (~30-100 KB each):**
```
cli/
  ├─ args.fn           # Argument parsing (flags, options, positional)
  ├─ colors.fn         # Terminal colors (red, green, bold, etc.)
  ├─ table.fn          # ASCII table formatting
  ├─ progress.fn       # Progress bars
  └─ prompt.fn         # Interactive prompts (readline-like)
```

**Testing Framework (~100 KB total):**
```
test/
  ├─ assert.fn         # assert_eq, assert_ok, assert_true, etc.
  ├─ mock.fn           # Mocking functions and actors
  └─ bench.fn          # Benchmarking utilities
```

**Utilities (~50-150 KB each):**
```
datetime.fn            # Date/time parsing, formatting, arithmetic
random.fn              # Random numbers, UUIDs
hash.fn                # MD5, SHA1, SHA256 hashing
base64.fn              # Base64 encoding/decoding
url.fn                 # URL parsing and building
log.fn                 # Structured logging (debug, info, warn, error)
math.fn                # Math functions (sin, cos, sqrt, pow, etc.)
```

**HTTP (~200-400 KB total):**
```
http/
  ├─ client.fn         # HTTP client (GET, POST, PUT, DELETE)
  ├─ server.fn         # HTTP server
  ├─ router.fn         # Request routing
  └─ websocket.fn      # WebSockets (future addition)
```

**Database (~2 MB):**
```
db/
  └─ sql.fn            # libSQL (SQLite-compatible) wrapper
```

**Concurrency (~400 KB for runtime + actors):**
```
concurrent/
  ├─ cache.fn          # In-memory cache actor
  ├─ queue.fn          # Job queue actor
  ├─ pubsub.fn         # Pub/sub actor
  └─ pool.fn           # Connection pool
```

**Security (~200-500 KB):**
```
crypto/
  ├─ hash.fn           # Cryptographic hashing (SHA256, etc.)
  ├─ aes.fn            # AES encryption
  ├─ rsa.fn            # RSA encryption
  └─ random.fn         # Cryptographically secure random
```

**Compression (~200-400 KB):**
```
compress/
  ├─ gzip.fn           # Gzip compression
  ├─ zlib.fn           # Zlib compression
  └─ tar.fn            # Tar archives
```

### Complete Module List

**Total: 45+ modules**

**Core (9 modules, always included):**
- list, map, set, option, result, string, int, float, bool

**Standard Library (36+ modules, tree-shaken):**

**I/O & System (6):**
- io.file, io.path, io.dir, io.env, io.process, os

**Data Formats (5):**
- json, csv, toml, yaml, xml

**Text (4):**
- regex, string.format, string.template, string.unicode

**CLI (5):**
- cli.args, cli.colors, cli.table, cli.progress, cli.prompt

**Testing (3):**
- test.assert, test.mock, test.bench

**Utilities (7):**
- datetime, random, hash, base64, url, log, math

**HTTP (4):**
- http.client, http.server, http.router, http.websocket

**Database (1):**
- db.sql

**Concurrency (4):**
- concurrent.cache, concurrent.queue, concurrent.pubsub, concurrent.pool

**Security (4):**
- crypto.hash, crypto.aes, crypto.rsa, crypto.random

**Compression (3):**
- compress.gzip, compress.zlib, compress.tar

### Binary Size Guide

Approximate binary sizes based on imports:

| What You Import | Binary Size | Use Case |
|-----------------|-------------|----------|
| Core only | 200 KB | Hello world |
| Core + io + cli | 400 KB | Basic CLI tool |
| + csv + regex | 600 KB | Data processing |
| + json + http.client | 900 KB | API consumer |
| + test | 1 MB | Tested application |
| + http.server + json | 1.5 MB | Simple web API |
| + db.sql | 3.5 MB | Web API with database |
| + concurrent.* | 4 MB | Actor-based server |
| Everything | ~6 MB | All features |

### Example Programs

#### **CSV Data Processor**

```
import io.file
import csv
import cli.colors

fn main() -> Result((), Error):
    let data = csv.read("sales.csv")?
    let total = data 
        |> list.fold(0, (sum, row) -> sum + row.get("amount"))
    
    print(cli.colors.green("Total sales: ${total}"))
    Ok(())

# Binary: ~650 KB
# Includes: core + io.file + csv + cli.colors
```

#### **GitHub API CLI**

```
import http.client
import json
import cli.args

fn main() -> Result((), Error):
    let args = cli.args.parse([
        Positional("username", "GitHub username")
    ])?
    
    let url = "https://api.github.com/users/{args.get("username")}"
    let user = http.client.get_json(url)?
    
    print("User: {user.get("login")}")
    print("Repos: {user.get("public_repos")}")
    Ok(())

# Binary: ~900 KB
# Includes: core + http.client + json + cli.args
```

#### **REST API with Database**

```
import http.server
import db.sql
import json
import log

type User derive(Json):
    id: Int
    name: String
    email: String

fn main() -> Result((), Error):
    log.info("Starting server...")
    
    let db = sql.open("app.db")?
    sql.execute(db, """
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            name TEXT,
            email TEXT
        )
    """, [])?
    
    http.serve(8080, handle(db))

fn handle(req: Request, db: Database) -> Response:
    match (req.method, req.path):
        (Get, "/users") ->
            let users = list_users(db)?
            Response.json(users)
        
        (Post, "/users") ->
            let user = req.body |> json.decode(User)?
            let created = create_user(db, user)?
            Response.created(created)
        
        _ -> Response.not_found()

# Binary: ~3.5 MB
# Includes: core + http.server + db.sql + json + log
```

#### **Build Tool**

```
import io.file
import io.process
import toml
import log
import cli.colors

fn main() -> Result((), Error):
    log.info("Reading build config...")
    let config = toml.parse(io.file.read("build.toml")?)?
    
    log.info("Compiling sources...")
    for src in config.get("sources"):
        io.process.exec("fern", ["build", src])?
        print(cli.colors.green("✓ Compiled {src}"))
    
    log.info("Build complete!")
    Ok(())

# Binary: ~800 KB
# Includes: core + io.file + io.process + toml + log + cli.colors
```

### Testing Framework

Built-in test runner with `fern test`:

```bash
# Run all tests
fern test

# Run specific test file
fern test tests/parser_test.fn

# Run with coverage
fern test --coverage

# Run benchmarks
fern test --bench

# Watch mode (rerun on changes)
fern test --watch
```

**Example test file:**

```
# tests/list_test.fn
import test.assert
import list

fn test_map():
    let result = list.map([1, 2, 3], (x) -> x * 2)
    assert_eq(result, [2, 4, 6], "should double values")

fn test_filter():
    let result = list.filter([1, 2, 3, 4], (x) -> x % 2 == 0)
    assert_eq(result, [2, 4], "should keep even numbers")

fn test_error_handling():
    let result = parse_int("not a number")
    assert_err(result, "should fail on invalid input")

# Benchmark
fn bench_map_large():
    let input = list.range(0, 10000)
    list.map(input, (x) -> x * 2)
```

**Test output:**

```
$ fern test
Running tests...
✓ test_map (0.2ms)
✓ test_filter (0.1ms)
✓ test_error_handling (0.1ms)

Running benchmarks...
bench_map_large: 1.5ms

All tests passed! (3/3)
```

### What's NOT in Standard Library

These belong in the package ecosystem:

- Web framework implementation: a first-party package under Decision124, rather
  than special compiler syntax or a mandatory CLI runtime dependency
- ❌ ORMs (ActiveRecord-style)
- ❌ Specific database drivers (Postgres, MySQL, MongoDB)
- ❌ GraphQL servers/clients
- ❌ Image processing
- ❌ PDF generation
- ❌ Email sending (SMTP)
- ❌ AWS/Cloud SDKs
- ❌ Machine learning libraries
- ❌ Game engines
- Native desktop GUI frameworks (the first-party browser UI is in Decision124)

**Rationale:** These features belong in focused packages so they can evolve
without expanding every program's core runtime. Decision124 makes the web/UI
framework first-party; the other specialized integrations remain ecosystem work.

### Package Ecosystem

For features beyond stdlib, use the package manager:

```toml
# fern.toml
[package]
name = "my-web-app"
version = "0.1.0"

[dependencies]
postgres = "0.5.0"      # PostgreSQL driver
template = "1.2.0"      # Advanced templating
auth = "0.3.0"          # Authentication helpers
```

The stdlib provides the foundation, packages provide specialization.

---

## Implementation

Fern is implemented as a Rust Cargo workspace. The compiler owns lexical analysis,
parsing, module resolution, type/Result checking, formatting, documentation, REPL,
LSP and typed machine lowering. Cranelift emits native objects; the platform linker
combines them with the Rust runtime archive. No external IR compiler is needed.

```text
Fern source → typed Rust frontend → machine IR → Cranelift object → host linker
                                                                    ↑
                                                           Rust runtime archive
```

The workspace separates `fern`, `fern-json`, `fern-runtime`,
`fern-runtime-native`, `fern-test-supervisor` and `xtask`, with optional
`fern-web-protocol`, `fern-web-app`, `fern-web`, `fern-browser` and
`fern-browser-worker` packages.
Native startup is isolated
from the runtime core so Rust tests can link its ABI without a duplicate main.
The shared JSON crate implements bounded parsing, exact numbers and immutable
values used by both the REPL and native runtime.

Native memory uses a Rust-owned nonmoving tracing collector with actor-owned
payload heaps, copied messages, compiler root frames, temporary roots and finalized
Rust values. Conservative stack/register and heap-word scanning remain enabled;
reference metadata remains bookkeeping. The WASM backend branches from checked
semantic IR before native lowering, preserving i64 integers and using a separate
bounded precise heap for strings, records, sums, lists, tuples, Option/Result and
rooted host handles. Complete precise native roots/layouts, browser closures/maps,
inferred borrowing and reuse remain implementation
work. See [runtime memory](docs/MEMORY_MANAGEMENT.md) and
the [full-stack target architecture](docs/FULL_STACK_ARCHITECTURE.md).

Fern-authored code and development tooling are Rust. Mature third-party native
libraries may be used through Rust wrappers: SQLite uses rusqlite; HTTP uses
ureq/rustls and Rust-wrapped crypto. Prefer native Rust dependencies when they
preserve the required behavior. Editor integration is the Rust LSP (`fern lsp`);
Tree-sitter and the related grammar package are removed.

The implementation targets macOS and Linux on ARM64 and x86-64, with acceptance
reported per tested platform. `cargo xtask check` verifies formatting, Clippy,
workspace tests, native outputs, compatibility, examples and fuzz invariants.
`cargo xtask package` validates the complete compiler/helper/runtime bundle.

Read [FERN_STYLE.md](FERN_STYLE.md), [CLAUDE.md](CLAUDE.md) and
[ROADMAP.md](ROADMAP.md) for development and current evidence. Earlier architecture
proposals are retained in the [historical implementation plan](docs/history/IMPLEMENTATION_PLAN.md).

## Summary

The table below summarizes the broader language vision, including unimplemented
features and performance targets. It is not a release capability or benchmark
claim; consult [release readiness](docs/RELEASE_READINESS.md).

| Feature | Syntax/Approach |
|---------|----------------|
| **Language** | |
| Name | Fern |
| CLI / Extension | `fern` / `.fn` |
| Syntax | Indentation-based (Python-style) |
| Philosophy | Functional-first, immutable, pragmatic |
| **Type System** | |
| Types | Static with inference, no null |
| Generics | `List(a)` with lowercase variables |
| Polymorphism | Traits with `trait`/`impl` |
| Type Safety | Newtypes, union types, `Option(a)` |
| Derivation | `derive(Show, Eq, Ord, Clone)` |
| **Language Features** | |
| Functions | Multiple clauses, pattern matching |
| Conditionals | `match` (unified), postfix guards |
| Pipes | `\|>` with `_` placeholder |
| Strings | Native interpolation `"{expr}"` |
| Errors | `Result(ok, err)`, `?` operator |
| **Concurrency** | |
| Model | Actor-based (Erlang/Elixir style) |
| Processes | Lightweight, isolated, typed messages |
| Message Passing | `Pid(msg)`, type-safe, immutable |
| Supervision | Link, monitor, automatic restart |
| Patterns | Cache, queue, pubsub (stdlib) |
| **Database** | |
| Embedded | libSQL (SQLite-compatible) |
| Modes | Local file OR remote connection |
| Scaling | Embedded → Turso managed hosting |
| Features | Migrations, transactions, replication |
| **Deployment** | |
| CLI Mode | < 1 MB, < 5ms startup, no runtime |
| Server Mode | 2-4 MB, actors + libSQL included |
| Output | Single static binary |
| Dependencies | Zero (truly standalone) |
| **Infrastructure Replacement** | |
| Cache | Actor-based (replaces Redis) |
| Queue | Actor-based (replaces RabbitMQ) |
| Database | libSQL (replaces PostgreSQL) |
| Deployment | One binary (replaces containers) |
| **Standard Library** | |
| Structure | Two-tier (Core + Tree-shaken) |
| Core Modules | 9 (list, map, option, result, etc.) |
| Stdlib Modules | 45+ (I/O, HTTP, DB, testing, etc.) |
| Testing | Built-in (assert, mock, bench) |
| Size Range | 200 KB (core) to 6 MB (everything) |
| Philosophy | Batteries-included without bloat |
