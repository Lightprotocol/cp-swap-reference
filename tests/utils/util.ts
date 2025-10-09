import * as anchor from "@coral-xyz/anchor";
import { web3 } from "@coral-xyz/anchor";
import {
  Connection,
  PublicKey,
  Keypair,
  Signer,
  TransactionInstruction,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  createMint as createSplMint,
  TOKEN_PROGRAM_ID,
  getOrCreateAssociatedTokenAccount,
  mintTo as mintToSpl,
  TOKEN_2022_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  ExtensionType,
  getMintLen,
  createInitializeTransferFeeConfigInstruction,
  createInitializeMintInstruction,
  getAccount,
  ASSOCIATED_TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { sendTransaction } from "./index";
import { CTOKEN_PROGRAM_ID, createRpc } from "@lightprotocol/stateless.js";
import {
  CompressedTokenProgram,
  createTokenPool,
  createMint as createCompressedMint,
  getAccountInterface,
  getMintInterface,
  getOrCreateAssociatedTokenAccountInterface,
  mintTo,
} from "@lightprotocol/compressed-token";

// anchor get provider
// TODO: add to anchor ts.
const rpc = createRpc();

// create a token mint and a token2022 mint with transferFeeConfig
export async function createTokenMintAndAssociatedTokenAccount(
  connection: Connection,
  payer: Signer,
  mintAuthority: Signer,
  transferFeeConfig: { transferFeeBasisPoints: number; MaxFee: number },
  token0Type: "ctoken" | "spl" | "token2022" = "ctoken",
  token1Type: "ctoken" | "spl" | "token2022" = "ctoken"
) {
  let ixs: TransactionInstruction[] = [];
  ixs.push(
    web3.SystemProgram.transfer({
      fromPubkey: payer.publicKey,
      toPubkey: mintAuthority.publicKey,
      lamports: web3.LAMPORTS_PER_SOL,
    })
  );
  await sendTransaction(connection, ixs, [payer]);

  interface Token {
    address: PublicKey;
    program: PublicKey;
    type: "ctoken" | "spl" | "token2022";
  }

  let tokenArray: Token[] = [];
  let token0: PublicKey;
  let token0Program: PublicKey;
  let token1: PublicKey;
  let token1Program: PublicKey;
  let finalToken0Type: "ctoken" | "spl" | "token2022";
  let finalToken1Type: "ctoken" | "spl" | "token2022";

  const rpc = createRpc();

  if (token0Type === "ctoken") {
    const { mint: mint0 } = await createCompressedMint(
      rpc,
      mintAuthority,
      mintAuthority,
      null,
      9,
      undefined,
      undefined,
      undefined
    );
    token0 = mint0;
    token0Program = CTOKEN_PROGRAM_ID;
  } else if (token0Type === "token2022") {
    token0 = await createSplMint(
      connection,
      mintAuthority,
      mintAuthority.publicKey,
      null,
      9,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    token0Program = TOKEN_2022_PROGRAM_ID;
  } else {
    token0 = await createSplMint(
      connection,
      mintAuthority,
      mintAuthority.publicKey,
      null,
      9
    );
    token0Program = TOKEN_PROGRAM_ID;
  }
  tokenArray.push({
    address: token0,
    program: token0Program,
    type: token0Type,
  });

  if (token1Type === "ctoken") {
    const { mint: mint1 } = await createCompressedMint(
      rpc,
      mintAuthority,
      mintAuthority,
      null,
      9,
      undefined,
      undefined,
      undefined
    );
    token1 = mint1;
    token1Program = CTOKEN_PROGRAM_ID;
  } else if (token1Type === "token2022") {
    token1 = await createSplMint(
      connection,
      mintAuthority,
      mintAuthority.publicKey,
      null,
      9,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    token1Program = TOKEN_2022_PROGRAM_ID;
  } else {
    token1 = await createSplMint(
      connection,
      mintAuthority,
      mintAuthority.publicKey,
      null,
      9
    );
    token1Program = TOKEN_PROGRAM_ID;
  }
  tokenArray.push({
    address: token1,
    program: token1Program,
    type: token1Type,
  });

  tokenArray.sort(function (x, y) {
    const buffer1 = x.address.toBuffer();
    const buffer2 = y.address.toBuffer();

    for (let i = 0; i < buffer1.length && i < buffer2.length; i++) {
      if (buffer1[i] < buffer2[i]) {
        return -1;
      }
      if (buffer1[i] > buffer2[i]) {
        return 1;
      }
    }

    if (buffer1.length < buffer2.length) {
      return -1;
    }
    if (buffer1.length > buffer2.length) {
      return 1;
    }

    return 0;
  });

  token0 = tokenArray[0].address;
  token1 = tokenArray[1].address;
  token0Program = tokenArray[0].program;
  token1Program = tokenArray[1].program;
  finalToken0Type = tokenArray[0].type;
  finalToken1Type = tokenArray[1].type;
  console.log("token0", token0.toBase58());
  console.log("token1", token1.toBase58());
  console.log("token0Program", token0Program.toBase58());
  console.log("token1Program", token1Program.toBase58());

  const ownerToken0Account = await getOrCreateAssociatedTokenAccountInterface(
    rpc,
    payer,
    token0,
    payer.publicKey,
    false,
    "processed",
    { skipPreflight: true },
    token0Program,
    finalToken0Type === "ctoken"
      ? CTOKEN_PROGRAM_ID
      : ASSOCIATED_TOKEN_PROGRAM_ID
  );

  console.log("ownerToken0Account", ownerToken0Account.address.toBase58());
  const ownerToken1Account = await getOrCreateAssociatedTokenAccountInterface(
    rpc,
    payer,
    token1,
    payer.publicKey,
    false,
    "processed",
    { skipPreflight: true },
    token1Program,
    finalToken1Type === "ctoken"
      ? CTOKEN_PROGRAM_ID
      : ASSOCIATED_TOKEN_PROGRAM_ID
  );

  console.log("ownerToken1Account", ownerToken1Account.address.toBase58());

  const queue = (await rpc.getStateTreeInfos())[0].queue;

  if (finalToken0Type === "ctoken") {
    await mintTo(
      rpc,
      payer,
      token0,
      ownerToken0Account.address,
      mintAuthority,
      100_000_000_000_000,
      queue,
      queue,
      { skipPreflight: true }
    );
  } else {
    await mintToSpl(
      connection,
      payer,
      token0,
      ownerToken0Account.address,
      mintAuthority,
      100_000_000_000_000,
      [],
      { skipPreflight: true },
      token0Program
    );
    console.log("mintToSpl token0", token0.toBase58());
    console.log(
      "mintToSpl ownerToken0Account",
      ownerToken0Account.address.toBase58()
    );
  }

  if (finalToken1Type === "ctoken") {
    await mintTo(
      rpc,
      payer,
      token1,
      ownerToken1Account.address,
      mintAuthority,
      100_000_000_000_000,
      queue,
      queue,
      { skipPreflight: true }
    );
  } else {
    await mintToSpl(
      connection,
      payer,
      token1,
      ownerToken1Account.address,
      mintAuthority,
      100_000_000_000_000,
      [],
      { skipPreflight: true },
      token1Program
    );
    console.log("mintToSpl token1", token1.toBase58());
    console.log(
      "mintToSpl ownerToken1Account",
      ownerToken1Account.address.toBase58()
    );
  }

  if (finalToken0Type !== "ctoken") {
    await createTokenPool(
      rpc,
      payer,
      token0,
      { skipPreflight: true },
      token0Program
    );
  }

  if (finalToken1Type !== "ctoken") {
    await createTokenPool(
      rpc,
      payer,
      token1,
      { skipPreflight: true },
      token1Program
    );
  }

  return [
    { token0, token0Program },
    { token1, token1Program },
  ];
}

async function createMintWithTransferFee(
  connection: Connection,
  payer: Signer,
  mintAuthority: Signer,
  mintKeypair = Keypair.generate(),
  transferFeeConfig: { transferFeeBasisPoints: number; MaxFee: number }
) {
  const transferFeeConfigAuthority = Keypair.generate();
  const withdrawWithheldAuthority = Keypair.generate();

  const extensions = [ExtensionType.TransferFeeConfig];

  const mintLen = getMintLen(extensions);
  const decimals = 9;

  const mintLamports = await connection.getMinimumBalanceForRentExemption(
    mintLen
  );
  const mintTransaction = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: mintKeypair.publicKey,
      space: mintLen,
      lamports: mintLamports,
      programId: TOKEN_2022_PROGRAM_ID,
    }),
    createInitializeTransferFeeConfigInstruction(
      mintKeypair.publicKey,
      transferFeeConfigAuthority.publicKey,
      withdrawWithheldAuthority.publicKey,
      transferFeeConfig.transferFeeBasisPoints,
      BigInt(transferFeeConfig.MaxFee),
      TOKEN_2022_PROGRAM_ID
    ),
    createInitializeMintInstruction(
      mintKeypair.publicKey,
      decimals,
      mintAuthority.publicKey,
      null,
      TOKEN_2022_PROGRAM_ID
    )
  );
  await sendAndConfirmTransaction(
    connection,
    mintTransaction,
    [payer, mintKeypair],
    undefined
  );

  return mintKeypair.publicKey;
}

export async function getUserAndPoolVaultAmount(
  owner: PublicKey,
  token0Mint: PublicKey,
  token0Program: PublicKey,
  token1Mint: PublicKey,
  token1Program: PublicKey,
  poolToken0Vault: PublicKey,
  poolToken1Vault: PublicKey
) {
  const ownerToken0AccountAddr = getAssociatedTokenAddressSync(
    token0Mint,
    owner,
    false,
    token0Program,
    token0Program.equals(CTOKEN_PROGRAM_ID)
      ? CTOKEN_PROGRAM_ID
      : ASSOCIATED_TOKEN_PROGRAM_ID
  );

  const ownerToken1AccountAddr = getAssociatedTokenAddressSync(
    token1Mint,
    owner,
    false,
    token1Program,
    token1Program.equals(CTOKEN_PROGRAM_ID)
      ? CTOKEN_PROGRAM_ID
      : ASSOCIATED_TOKEN_PROGRAM_ID
  );

  const ownerToken0Account = await getAccountInterface(
    rpc,
    ownerToken0AccountAddr,
    "processed",
    token0Program
  );

  const ownerToken1Account = await getAccountInterface(
    rpc,
    ownerToken1AccountAddr,
    "processed",
    token1Program
  );

  const poolVault0TokenAccount = await getAccount(
    anchor.getProvider().connection,
    poolToken0Vault,
    "processed",
    CompressedTokenProgram.programId
  );
  const poolVault1TokenAccount = await getAccount(
    anchor.getProvider().connection,
    poolToken1Vault,
    "processed",
    CompressedTokenProgram.programId
  );
  return {
    ownerToken0Account,
    ownerToken1Account,
    poolVault0TokenAccount,
    poolVault1TokenAccount,
  };
}

export async function getUserAndPoolLpAmount(
  owner: PublicKey,
  lpMint: PublicKey,
  lpVault: PublicKey
) {
  const userLpTokenAddr = getAssociatedTokenAddressSync(
    lpMint,
    owner,
    undefined,
    CTOKEN_PROGRAM_ID,
    CTOKEN_PROGRAM_ID
  );

  const userLpAccount = await getAccountInterface(
    rpc,
    userLpTokenAddr,
    "processed",
    CTOKEN_PROGRAM_ID
  );

  const poolLpVaultAccount = await getAccount(
    anchor.getProvider().connection,
    lpVault,
    "processed",
    CTOKEN_PROGRAM_ID
  );

  return {
    userLpAccount,
    poolLpVaultAccount,
  };
}

export function isEqual(amount1: bigint, amount2: bigint) {
  if (
    BigInt(amount1) === BigInt(amount2) ||
    BigInt(amount1) - BigInt(amount2) === BigInt(1) ||
    BigInt(amount1) - BigInt(amount2) === BigInt(-1)
  ) {
    return true;
  }
  return false;
}
