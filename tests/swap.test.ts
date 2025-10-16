import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { RaydiumCpSwap } from "../target/types/raydium_cp_swap";
import { setupSwapTest, swap_base_input, swap_base_output } from "./utils";
import { assert } from "chai";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import {
  getAccountInterface,
  getAtaProgramId,
} from "@lightprotocol/compressed-token";
import { createRpc, CTOKEN_PROGRAM_ID } from "@lightprotocol/stateless.js";

describe.only("swap test", () => {
  anchor.setProvider(anchor.AnchorProvider.env());
  const rpc = createRpc();
  const owner = anchor.Wallet.local().payer;

  const program = anchor.workspace.RaydiumCpSwap as Program<RaydiumCpSwap>;

  const confirmOptions = {
    skipPreflight: true,
  };

  it.only("swap base input without transfer fee", async () => {
    const { configAddress, poolAddress, poolState } = await setupSwapTest(
      program,
      rpc,
      owner,
      {
        config_index: 0,
        tradeFeeRate: new BN(10),
        protocolFeeRate: new BN(1000),
        fundFeeRate: new BN(25000),
        create_fee: new BN(0),
      },
      { transferFeeBasisPoints: 0, MaxFee: 0 },
      confirmOptions
    );
    const inputToken = poolState.token0Mint;
    const inputTokenProgram = poolState.token0Program;
    const inputTokenAccountAddr = getAssociatedTokenAddressSync(
      inputToken,
      owner.publicKey,
      false,
      inputTokenProgram,
      getAtaProgramId(inputTokenProgram)
    );
    const { parsed: inputTokenAccountBefore } = await getAccountInterface(
      rpc,
      inputTokenAccountAddr,
      "processed",
      inputTokenProgram
    );
    await sleep(1000);
    let amount_in = new BN(100000000);

    await swap_base_input(
      program,
      owner,
      configAddress,
      inputToken,
      inputTokenProgram,
      poolState.token1Mint,
      poolState.token1Program,
      amount_in,
      new BN(0),
      confirmOptions
    );
    const { parsed: inputTokenAccountAfter } = await getAccountInterface(
      rpc,
      inputTokenAccountAddr,
      "processed",
      inputTokenProgram
    );
    assert.equal(
      inputTokenAccountBefore.amount - inputTokenAccountAfter.amount,
      BigInt(amount_in.toString())
    );
  });

  it("swap base output without transfer fee", async () => {
    const { configAddress, poolAddress, poolState } = await setupSwapTest(
      program,
      anchor.getProvider().connection,
      owner,
      {
        config_index: 0,
        tradeFeeRate: new BN(10),
        protocolFeeRate: new BN(1000),
        fundFeeRate: new BN(25000),
        create_fee: new BN(0),
      },
      { transferFeeBasisPoints: 0, MaxFee: 0 }
    );
    const inputToken = poolState.token0Mint;
    const inputTokenProgram = poolState.token0Program;
    const inputTokenAccountAddr = getAssociatedTokenAddressSync(
      inputToken,
      owner.publicKey,
      false,
      inputTokenProgram,
      getAtaProgramId(inputTokenProgram)
    );
    const outputToken = poolState.token1Mint;
    const outputTokenProgram = poolState.token1Program;
    const outputTokenAccountAddr = getAssociatedTokenAddressSync(
      outputToken,
      owner.publicKey,
      false,
      outputTokenProgram,
      getAtaProgramId(outputTokenProgram)
    );
    const { parsed: outputTokenAccountBefore } = await getAccountInterface(
      rpc,
      outputTokenAccountAddr,
      "processed",
      outputTokenProgram
    );
    await sleep(1000);
    let amount_out = new BN(100000000);
    await swap_base_output(
      program,
      owner,
      configAddress,
      inputToken,
      inputTokenProgram,
      poolState.token1Mint,
      poolState.token1Program,
      amount_out,
      new BN(10000000000000),
      confirmOptions
    );
    const { parsed: outputTokenAccountAfter } = await getAccountInterface(
      rpc,
      outputTokenAccountAddr,
      "processed",
      outputTokenProgram
    );
    assert.equal(
      outputTokenAccountAfter.amount - outputTokenAccountBefore.amount,
      BigInt(amount_out.toString())
    );
  });

  it("swap base output with transfer fee", async () => {
    const transferFeeConfig = { transferFeeBasisPoints: 5, MaxFee: 5000 }; // %5
    const { configAddress, poolAddress, poolState } = await setupSwapTest(
      program,
      rpc,
      owner,
      {
        config_index: 0,
        tradeFeeRate: new BN(10),
        protocolFeeRate: new BN(1000),
        fundFeeRate: new BN(25000),
        create_fee: new BN(0),
      },
      transferFeeConfig
    );

    const inputToken = poolState.token0Mint;
    const inputTokenProgram = poolState.token0Program;
    const inputTokenAccountAddr = getAssociatedTokenAddressSync(
      inputToken,
      owner.publicKey,
      false,
      inputTokenProgram,
      getAtaProgramId(inputTokenProgram)
    );
    const outputToken = poolState.token1Mint;
    const outputTokenProgram = poolState.token1Program;
    const outputTokenAccountAddr = getAssociatedTokenAddressSync(
      outputToken,
      owner.publicKey,
      false,
      outputTokenProgram,
      getAtaProgramId(outputTokenProgram)
    );
    const { parsed: outputTokenAccountBefore } = await getAccountInterface(
      rpc,
      outputTokenAccountAddr,
      "processed",
      outputTokenProgram
    );
    await sleep(1000);
    let amount_out = new BN(100000000);
    await swap_base_output(
      program,
      owner,
      configAddress,
      inputToken,
      inputTokenProgram,
      poolState.token1Mint,
      poolState.token1Program,
      amount_out,
      new BN(10000000000000),
      confirmOptions
    );
    const { parsed: outputTokenAccountAfter } = await getAccountInterface(
      rpc,
      outputTokenAccountAddr,
      "processed",
      outputTokenProgram
    );
    assert.equal(
      outputTokenAccountAfter.amount - outputTokenAccountBefore.amount,
      BigInt(amount_out.toString())
    );
  });
});

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
