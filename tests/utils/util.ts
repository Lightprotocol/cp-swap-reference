import { web3 } from "@coral-xyz/anchor";
import {
  Connection,
  PublicKey,
  Keypair,
  Signer,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  ExtensionType,
  getMintLen,
  createInitializeTransferFeeConfigInstruction,
  createInitializeMintInstruction,
  ASSOCIATED_TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { sendTransaction } from "./index";
import { CTOKEN_PROGRAM_ID, createRpc } from "@lightprotocol/stateless.js";
import {
  CompressedTokenProgram,
  createMint,
  createMintSPL,
  getAccountInterface,
  getAtaInterface,
  getAtaProgramId,
  getOrCreateAssociatedTokenAccountInterface,
  mintToInterface,
} from "@lightprotocol/compressed-token";

const rpc = createRpc();

export async function createTokenMintAndAssociatedTokenAccount(
  connection: Connection,
  payer: Signer,
  mintAuthority: Signer,
  transferFeeConfig: { transferFeeBasisPoints: number; MaxFee: number },
  token0Type: "ctoken" | "spl" | "token2022" = "ctoken",
  token1Type: "ctoken" | "spl" | "token2022" = "token2022"
) {
  await sendTransaction(
    connection,
    [
      web3.SystemProgram.transfer({
        fromPubkey: payer.publicKey,
        toPubkey: mintAuthority.publicKey,
        lamports: web3.LAMPORTS_PER_SOL,
      }),
    ],
    [payer]
  );

  let token0: PublicKey;
  let token0Program: PublicKey;
  let token1: PublicKey;
  let token1Program: PublicKey;

  if (token0Type === "ctoken") {
    const { mint } = await createMint(
      rpc,
      mintAuthority,
      mintAuthority,
      null,
      9
    );
    token0 = mint;
    token0Program = CTOKEN_PROGRAM_ID;
  } else if (token0Type === "token2022") {
    const { mint } = await createMintSPL(
      rpc,
      mintAuthority,
      mintAuthority.publicKey,
      null,
      9,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    token0 = mint;
    token0Program = TOKEN_2022_PROGRAM_ID;
  } else {
    const { mint } = await createMintSPL(
      rpc,
      mintAuthority,
      mintAuthority.publicKey,
      null,
      9,
      undefined,
      undefined,
      TOKEN_PROGRAM_ID
    );
    token0 = mint;
    token0Program = TOKEN_PROGRAM_ID;
  }

  if (token1Type === "ctoken") {
    const { mint } = await createMint(
      rpc,
      mintAuthority,
      mintAuthority,
      null,
      9
    );
    token1 = mint;
    token1Program = CTOKEN_PROGRAM_ID;
  } else if (token1Type === "token2022") {
    const { mint } = await createMintSPL(
      rpc,
      mintAuthority,
      mintAuthority.publicKey,
      null,
      9,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    token1 = mint;
    token1Program = TOKEN_2022_PROGRAM_ID;
  } else {
    const { mint } = await createMintSPL(
      rpc,
      mintAuthority,
      mintAuthority.publicKey,
      null,
      9,
      undefined,
      undefined,
      TOKEN_PROGRAM_ID
    );
    token1 = mint;
    token1Program = TOKEN_PROGRAM_ID;
  }

  const tokenArray = [
    { address: token0, program: token0Program, type: token0Type },
    { address: token1, program: token1Program, type: token1Type },
  ];

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

  const token0Info = tokenArray[0];
  const token1Info = tokenArray[1];

  const getAssociatedProgram = (type: string) =>
    type === "ctoken" ? CTOKEN_PROGRAM_ID : ASSOCIATED_TOKEN_PROGRAM_ID;

  const [ownerToken0Account, ownerToken1Account] = await Promise.all([
    getOrCreateAssociatedTokenAccountInterface(
      rpc,
      payer,
      token0Info.address,
      payer.publicKey,
      false,
      "processed",
      { skipPreflight: true },
      token0Info.program,
      getAssociatedProgram(token0Info.type)
    ),
    getOrCreateAssociatedTokenAccountInterface(
      rpc,
      payer,
      token1Info.address,
      payer.publicKey,
      false,
      "processed",
      { skipPreflight: true },
      token1Info.program,
      getAssociatedProgram(token1Info.type)
    ),
  ]);

  await Promise.all([
    mintToInterface(
      rpc,
      payer,
      token0Info.address,
      ownerToken0Account.address,
      mintAuthority,
      100_000_000_000_000,
      [],
      { skipPreflight: true }
    ),
    mintToInterface(
      rpc,
      payer,
      token1Info.address,
      ownerToken1Account.address,
      mintAuthority,
      100_000_000_000_000,
      [],
      { skipPreflight: true }
    ),
  ]);

  return [
    { token0: token0Info.address, token0Program: token0Info.program },
    { token1: token1Info.address, token1Program: token1Info.program },
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
    getAtaProgramId(token0Program)
  );

  const ownerToken1AccountAddr = getAssociatedTokenAddressSync(
    token1Mint,
    owner,
    false,
    token1Program,
    getAtaProgramId(token1Program)
  );

  const ownerToken0Account = await getAtaInterface(
    rpc,
    owner,
    token0Mint,
    "processed",
    token0Program
  );

  const ownerToken1Account = await getAtaInterface(
    rpc,
    owner,
    token1Mint,
    "processed",
    token1Program
  );

  const poolVault0TokenAccount = await getAccountInterface(
    rpc,
    poolToken0Vault,
    "processed",
    CompressedTokenProgram.programId
  );
  const poolVault1TokenAccount = await getAccountInterface(
    rpc,
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

  const userLpAccount = await getAtaInterface(
    rpc,
    owner,
    lpMint,
    "processed",
    CTOKEN_PROGRAM_ID
  );

  const poolLpVaultAccount = await getAccountInterface(
    rpc,
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
