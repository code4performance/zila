# Zila Lang in diagrams

Diagrams that explain how Zila Lang works (GitHub draws them). They were moved here from the README to keep it short.

## 1. Errors are values

A function that can fail says so in its type. `try` is the one visible place where a failure travels up.

```mermaid
flowchart TD
    CALL["let n = try parse(s)"] --> R{"parse returns<br/>int or error"}
    R -- "ok(v)" --> USE["n = v<br/>execution continues"]
    R -- "err(e)" --> UP["current function returns e<br/>(defers run, memory is freed)"]
    UP --> CALLER["caller receives the error<br/>e.kind, e.message, e.cause"]
    CALLER --> M{"match result"}
    M -- "ok(v)" --> HANDLE1["use the value"]
    M -- "err(e)" --> HANDLE2["handle it, or try again upward"]
```

## 2. Memory without a garbage collector

Values are reference counted and freed the instant the last reference goes away. Writes copy first when a value is shared.

```mermaid
flowchart TD
    A["let a = [1, 2, 3]<br/>count = 1"] --> B["let b = a<br/>count = 2 (cheap, no copy)"]
    B --> W{"write to b?"}
    W -- "yes, still shared" --> COPY["copy on write<br/>b gets its own list, a.count = 1"]
    W -- "no" --> SHARE["a and b share one list"]
    COPY --> END1["scope ends: each count drops"]
    SHARE --> END1
    END1 --> FREE["count = 0: freed immediately<br/>(also on return, break, try)"]
    FREE --> DROP["records with drop()<br/>close files and sockets"]
```

```mermaid
flowchart LR
    P["Parent<br/>new Node"] -- "strong Rc" --> C["Child<br/>new Node"]
    C -. "Weak: weak(parent)" .-> P
    C --> U["w.upgrade()<br/>returns Node? (none if freed)"]
```

A cycle made only of strong `Rc` references is not collected; break it with `Weak`.

## 3. Concurrency: tasks on one scheduler

Tasks are lightweight and cooperative. `await` hands the thread to another task instead of blocking it.

```mermaid
sequenceDiagram
    participant M as main
    participant S as Scheduler (Zila Lang code)
    participant A as double(5)
    participant B as double(6)
    M->>S: let a = double(5)
    M->>S: let b = double(6)
    S->>A: run
    A->>S: await sleep(100)
    S->>B: run
    B->>S: await sleep(100)
    Note over S: both timers wait together<br/>about 100 ms in total, not 200
    S-->>A: timer fired, resume
    S-->>B: timer fired, resume
    M->>S: await a, await b
    S-->>M: 10 and 12
```

## 4. Compile-time code generation

A `comptime func` is a native program that reads a type and writes Zila Lang source, which the compiler compiles in place.

```mermaid
flowchart LR
    T["type User = record<br/>name: text, age: int"] --> ATTR["#[derive(describe)]"]
    ATTR --> REF["Type value<br/>name, kind, fields,<br/>variants, attributes"]
    REF --> GENF["comptime func describe(t: Type)<br/>(checked subset, time and output limits)"]
    GENF --> SRC2["generated Zila Lang source"]
    SRC2 --> BACK["compiled with your code<br/>errors point at your code"]
    GENF -. "result cache on disk" .-> GENF
```

## 5. One language, from app to metal

```mermaid
flowchart TB
    L["Zila Lang<br/>one syntax, one compiler"]
    L --> APP["Everyday programs<br/>text, lists, maps, generics"]
    L --> NET["Concurrent programs<br/>spawn, async/await, net, tls, fs"]
    L --> GUI["Native windows<br/>use ui (Win32 or X11)"]
    L --> FFI["C bindings<br/>extern, #[c_layout], unsafe"]
    L --> OS["Kernel and boot sector<br/>asm, #[naked], no runtime"]
```
