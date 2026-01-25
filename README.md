# Rentfree AMM example

Fork of Raydium AMM that creates markets without paying rent-exemption.

- drop-in SDK, minimal code diff
- no extra CU overhead on hot paths
- no UX diff on hot paths

The SDK sponsors rent-exemption on behalf of your users for:
- PoolState
- Token Vaults
- LP Mint account
- User ATAs

Upgrading your program accounts to be rent-free is fast and straightforward because Light-token is a superset of SPL-token. See [here](https://www.zkcompression.com/light-token/defi/programs) for a guide.

For hands-on support, join the [Developer Discord](https://discord.com/invite/7cJ8BhAXhu).

## Environment Setup

1. Install `Rust`

   ```shell
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup default 1.81.0
   ```

2. Install `Solana `

   ```shell
   sh -c "$(curl -sSfL https://release.anza.xyz/v2.1.0/install)"
   ```

   then run `solana-keygen new` to create a keypair at the default location.

3. install `Anchor`

   ```shell
   # Installing using Anchor version manager (avm)
   cargo install --git https://github.com/coral-xyz/anchor avm --locked --force
   # Install anchor
   avm install 0.31.1
   ```

## Quickstart

1. install the latest compression dependencies

```shell
npm i -g @lightprotocol/zk-compression-cli@alpha --force

cargo install --git https://github.com/lightprotocol/photon.git --rev 6ba6813 --locked --force
```

2. Clone the repository and install node dependencies

```shell

git clone https://github.com/Lightprotocol/cp-swap-reference

cd raydium-cp-swap && yarn
```

3. Run the tests

```shell
yarn test
```

## License

Raydium constant product swap is licensed under the Apache License, Version 2.0.
