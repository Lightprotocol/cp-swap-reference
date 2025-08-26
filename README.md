# raydium-cp-swap

An AMM reference implementation based on Raydium's CP AMM.

We added:

- rent-free

Original readme:

- No Openbook market ID is required for pool creation
- Token22 is supported
- Built-in price oracle

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

git clone https://github.com/raydium-io/raydium-cp-swap

cd raydium-cp-swap && yarn
```

3. Run the tests

```shell
yarn test
```

## License

Raydium constant product swap is licensed under the Apache License, Version 2.0.
