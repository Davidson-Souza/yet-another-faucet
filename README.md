# yet another testnet faucet

This is a simple faucet for private signets that's easy to set-up and use. It suports lighting channel leasing and sending sats to an address. It intentionally doesn't check for abuse, like asking for fudns too often, so avoid exposing it to the internet.

To use ln you need to run a local signet CLN node and fund it with some sats. Compile with `features ln` to suport that.

## API

You can use your own front-end or script, just hit the /send/ route with a json object containing and address and amount. This rout returns a txid on success.

### Running

```bash
$ cargo run --release --features ln
```
