# Contributing

## Setup

Install Node:

```bash
brew install node
```

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Update your rc file to include this:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Then:

```bash
rustup target add wasm32-unknown-unknown
npx wrangler secret put USU_API_KEY
```

## Local dev

Edit your secrets and other local variables:

```bash
cat .dev.vars.example > .dev.vars
```

Start developing:

```bash
npx wrangler dev
```

## Previews

Create a preview for a new branch:

```bash
npx wrangler preview
```
