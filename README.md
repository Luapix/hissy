# hissy
A toy VM-based language implemented in Rust

This is a work-in-progress bytecode compiler and virtual machine for a programming language tentatively named Hissy.

The syntax looks like this at the moment, though it is likely to change:
```js
let make_counter(n):
  let n = 0
  let count():
    n = n + 1
    return n
  return count

let f = make_counter(6)
log f
```

This crate can be used as a library, or through its command line interface. To "install" the CLI, clone the repository, run `cargo build --release`, and move `target/release/hissy` somehere that's in your PATH.

<pre>
$ hissy compile hello.hsy    # compile hello.hsy to hello.hic
$ hissy run hello.hic        # run a bytecode file
$ hissy interpret hello.hsy  # compile & run at once
$ hissy lex/parse hello.hsy  # inspect the tokens / parse tree generated from a file
$ hissy list hello.hic       # inspect a bytecode file
</pre>
