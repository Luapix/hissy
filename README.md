# hissy (discontinued)
A toy VM-based language implemented in Rust

This is a work-in-progress bytecode compiler and virtual machine for a programming language tentatively named Hissy.

The syntax looks like this at the moment, though it is likely to change:
```js
let primes = [2]

let isPrime(n: Int) -> Bool:
	let i = 0
	let mayBePrime = true
	while i < primes.size() and primes[i]*primes[i] <= n and mayBePrime:
		if n % primes[i] == 0:
			mayBePrime = false
		i = i + 1
	return mayBePrime

let n = 3
while n <= 1000:
	if isPrime(n):
		primes.add(n)
		log(n)
	n = n + 2
```

This crate can be used as a library, or through its command line interface. To "install" the CLI, clone the repository, run `cargo build --release`, and move `target/release/hissy` somehere that's in your PATH.

<pre>
Usage:
  hissy lex|parse <src>
  hissy compile [--strip] [-o <bytecode>] <src>
  hissy list <bytecode>
  hissy run <bytecode>
  hissy interpret <src>
  hissy --help|--version

Arguments:
  <src>        Path to a Hissy source file (usually .hsy)
  <bytecode>   Path to a Hissy bytecode file (usually .hsyc)

Options:
  --strip      Strip debug symbols from output
  -o           Specifies the path of the resulting bytecode
  --help       Print this help message
  --version    Print the version
</pre>
