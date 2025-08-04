import { Program, BN, web3 } from "@coral-xyz/anchor";
import { RaydiumCpSwap } from "../../target/types/raydium_cp_swap";
import {
  Connection,
  ConfirmOptions,
  PublicKey,
  Keypair,
  Signer,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  TransactionMessage,
  SendTransactionError,
  ComputeBudgetProgram,
  AccountInfo,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  ASSOCIATED_TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import {
  accountExist,
  sendTransaction,
  getAmmConfigAddress,
  getAuthAddress,
  getPoolAddress,
  getPoolLpMintAddress,
  getPoolVaultAddress,
  createTokenMintAndAssociatedTokenAccount,
  getOrcleAccountAddress,
} from "./index";
process.env.LIGHT_PROTOCOL_VERSION = "V2";

import {
  createRpc,
  bn,
  TreeType,
  TreeInfo,
  sendAndConfirmTx,
  featureFlags,
  VERSION,
  Rpc,
  deriveAddressV2,
  SystemAccountMetaConfig,
  PackedAccounts,
  initializeCompressionConfig,
} from "@lightprotocol/stateless.js";

featureFlags.version = VERSION.V2;

import { ASSOCIATED_PROGRAM_ID } from "@coral-xyz/anchor/dist/cjs/utils/token";

export async function setupInitializeTest(
  program: Program<RaydiumCpSwap>,
  connection: Rpc,
  owner: Signer,
  config: {
    config_index: number;
    tradeFeeRate: BN;
    protocolFeeRate: BN;
    fundFeeRate: BN;
    create_fee: BN;
  },
  transferFeeConfig: { transferFeeBasisPoints: number; MaxFee: number } = {
    transferFeeBasisPoints: 0,
    MaxFee: 0,
  },
  confirmOptions?: ConfirmOptions
) {
  const [{ token0, token0Program }, { token1, token1Program }] =
    await createTokenMintAndAssociatedTokenAccount(
      connection,
      owner,
      new Keypair(),
      transferFeeConfig
    );
  const configAddress = await createAmmConfig(
    program,
    connection,
    owner,
    config.config_index,
    config.tradeFeeRate,
    config.protocolFeeRate,
    config.fundFeeRate,
    config.create_fee,
    confirmOptions
  );

  const txId = await initializeCompressionConfig(
    program.programId,
    connection,
    owner,
    program.provider.wallet.payer,
    100,
    new PublicKey("CLEuMG7pzJX9xAuKCFzBP154uiG1GaNo4Fq7x6KAcAfG"),
    [new PublicKey("EzKE84aVTkCUhDHLELqyJaq1Y7UVVmqxXqZjVHwHY3rK")]
  );
  console.log("compression config txId: ", txId);

  return {
    configAddress,
    token0,
    token0Program,
    token1,
    token1Program,
  };
}

export async function setupDepositTest(
  program: Program<RaydiumCpSwap>,
  connection: Connection,
  owner: Signer,
  config: {
    config_index: number;
    tradeFeeRate: BN;
    protocolFeeRate: BN;
    fundFeeRate: BN;
    create_fee: BN;
  },
  transferFeeConfig: { transferFeeBasisPoints: number; MaxFee: number } = {
    transferFeeBasisPoints: 0,
    MaxFee: 0,
  },
  confirmOptions?: ConfirmOptions,
  initAmount: { initAmount0: BN; initAmount1: BN } = {
    initAmount0: new BN(10000000000),
    initAmount1: new BN(20000000000),
  },
  tokenProgramRequired?: {
    token0Program: PublicKey;
    token1Program: PublicKey;
  }
) {
  const configAddress = await createAmmConfig(
    program,
    connection,
    owner,
    config.config_index,
    config.tradeFeeRate,
    config.protocolFeeRate,
    config.fundFeeRate,
    config.create_fee,
    confirmOptions
  );

  while (1) {
    const [{ token0, token0Program }, { token1, token1Program }] =
      await createTokenMintAndAssociatedTokenAccount(
        connection,
        owner,
        new Keypair(),
        transferFeeConfig
      );

    if (tokenProgramRequired != undefined) {
      if (
        token0Program.equals(tokenProgramRequired.token0Program) &&
        token1Program.equals(tokenProgramRequired.token1Program)
      ) {
        return await initialize(
          program,
          owner,
          configAddress,
          token0,
          token0Program,
          token1,
          token1Program,
          confirmOptions,
          initAmount
        );
      }
    } else {
      return await initialize(
        program,
        owner,
        configAddress,
        token0,
        token0Program,
        token1,
        token1Program,
        confirmOptions,
        initAmount
      );
    }
  }
}

export async function setupSwapTest(
  program: Program<RaydiumCpSwap>,
  connection: Connection,
  owner: Signer,
  config: {
    config_index: number;
    tradeFeeRate: BN;
    protocolFeeRate: BN;
    fundFeeRate: BN;
    create_fee: BN;
  },
  transferFeeConfig: { transferFeeBasisPoints: number; MaxFee: number } = {
    transferFeeBasisPoints: 0,
    MaxFee: 0,
  },
  confirmOptions?: ConfirmOptions
) {
  const configAddress = await createAmmConfig(
    program,
    connection,
    owner,
    config.config_index,
    config.tradeFeeRate,
    config.protocolFeeRate,
    config.fundFeeRate,
    config.create_fee,
    confirmOptions
  );

  const [{ token0, token0Program }, { token1, token1Program }] =
    await createTokenMintAndAssociatedTokenAccount(
      connection,
      owner,
      new Keypair(),
      transferFeeConfig
    );

  const { poolAddress, poolState } = await initialize(
    program,
    owner,
    configAddress,
    token0,
    token0Program,
    token1,
    token1Program,
    confirmOptions
  );

  await deposit(
    program,
    owner,
    poolState.ammConfig,
    poolState.token0Mint,
    poolState.token0Program,
    poolState.token1Mint,
    poolState.token1Program,
    new BN(10000000000),
    new BN(100000000000),
    new BN(100000000000),
    confirmOptions
  );
  return {
    configAddress: poolState.ammConfig,
    poolAddress,
    poolState,
  };
}

export async function createAmmConfig(
  program: Program<RaydiumCpSwap>,
  connection: Connection,
  owner: Signer,
  config_index: number,
  tradeFeeRate: BN,
  protocolFeeRate: BN,
  fundFeeRate: BN,
  create_fee: BN,
  confirmOptions?: ConfirmOptions
): Promise<PublicKey> {
  const [address, _] = await getAmmConfigAddress(
    config_index,
    program.programId
  );
  if (await accountExist(connection, address)) {
    return address;
  }

  const ix = await program.methods
    .createAmmConfig(
      config_index,
      tradeFeeRate,
      protocolFeeRate,
      fundFeeRate,
      create_fee
    )
    .accounts({
      owner: owner.publicKey,
      ammConfig: address,
      systemProgram: SystemProgram.programId,
    })
    .instruction();

  await sendTransaction(connection, [ix], [owner], confirmOptions);

  return address;
}

/**
 * Derive the compression config address for a given program id and config
 * index.
 * @param programId     The program id to derive the config for.
 * @param configIndex   Index. Default = 0.
 * @returns             The compression config address.
 */
export function deriveCompressionConfigAddress(
  programId: PublicKey,
  configIndex: number = 0
) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("compressible_config"), Buffer.from([configIndex])],
    programId
  )[0];
}

/**
 * Get the program data account address and its raw data for a given program.
 * @param program     The program to check.
 * @param connection  The connection to use.
 * @returns           The program data address and its account data.
 */
export async function getProgramDataAccount(
  program: Program<RaydiumCpSwap>,
  connection: Connection
): Promise<{
  programDataAddress: PublicKey;
  programDataAccountInfo: AccountInfo<Buffer>;
}> {
  const programAccount = await connection.getAccountInfo(program.programId);
  if (!programAccount) {
    throw new Error("Program account does not exist");
  }
  const programDataAddress = new PublicKey(programAccount.data.slice(4, 36));
  const programDataAccountInfo = await connection.getAccountInfo(
    programDataAddress
  );
  if (!programDataAccountInfo) {
    throw new Error("Program data account does not exist");
  }
  return { programDataAddress, programDataAccountInfo };
}

/**
 * Check that the provided authority matches the program's upgrade authority.
 * Throws if not matching or if no authority is set.
 * @param data                The raw data of the program data account.
 * @param providedAuthority   The expected upgrade authority public key.
 */
export function checkProgramUpdateAuthority(
  programDataAccountInfo: AccountInfo<Buffer>,
  providedAuthority: PublicKey
): void {
  // Check discriminator (should be 3 for ProgramData)
  const discriminator = programDataAccountInfo.data.readUInt32LE(0);
  if (discriminator !== 3) {
    throw new Error("Invalid program data discriminator");
  }
  // Check if authority exists
  const hasAuthority = programDataAccountInfo.data[12] === 1;
  if (!hasAuthority) {
    throw new Error("Program has no upgrade authority");
  }
  // Extract upgrade authority (bytes 13-44)
  const authorityBytes = programDataAccountInfo.data.slice(13, 45);
  const upgradeAuthority = new PublicKey(authorityBytes);
  if (!upgradeAuthority.equals(providedAuthority)) {
    throw new Error(
      `Provided authority ${providedAuthority.toBase58()} does not match program's upgrade authority ${upgradeAuthority.toBase58()}`
    );
  }
}

export async function initialize(
  program: Program<RaydiumCpSwap>,
  creator: Signer,
  configAddress: PublicKey,
  token0: PublicKey,
  token0Program: PublicKey,
  token1: PublicKey,
  token1Program: PublicKey,
  confirmOptions?: ConfirmOptions,
  initAmount: { initAmount0: BN; initAmount1: BN } = {
    initAmount0: new BN(10000000000),
    initAmount1: new BN(20000000000),
  },
  createPoolFee = new PublicKey("DNXgeM9EiiaAbaWvwjHj9fQQLAX5ZsfHyvmYUNRAdNC8")
) {
  // Create RPC client for compression
  const rpc = createRpc();

  // Get Tree Infos
  const addressTreeInfo: TreeInfo = {
    tree: new PublicKey("EzKE84aVTkCUhDHLELqyJaq1Y7UVVmqxXqZjVHwHY3rK"),
    queue: new PublicKey("EzKE84aVTkCUhDHLELqyJaq1Y7UVVmqxXqZjVHwHY3rK"),
    cpiContext: null,
    treeType: TreeType.AddressV2,
    nextTreeInfo: null,
  };
  const stateTreeInfo: TreeInfo = {
    tree: new PublicKey("HLKs5NJ8FXkJg8BrzJt56adFYYuwg5etzDtBbQYTsixu"),
    queue: new PublicKey("6L7SzhYB3anwEQ9cphpJ1U7Scwj57bx2xueReg7R9cKU"),
    cpiContext: PublicKey.default,
    treeType: TreeType.StateV2,
    nextTreeInfo: null,
  };

  const [auth] = await getAuthAddress(program.programId);
  const [poolAddress] = await getPoolAddress(
    configAddress,
    token0,
    token1,
    program.programId
  );
  const [lpMintAddress] = await getPoolLpMintAddress(
    poolAddress,
    program.programId
  );
  const [vault0] = await getPoolVaultAddress(
    poolAddress,
    token0,
    program.programId
  );
  const [vault1] = await getPoolVaultAddress(
    poolAddress,
    token1,
    program.programId
  );
  const [creatorLpTokenAddress] = await PublicKey.findProgramAddress(
    [
      creator.publicKey.toBuffer(),
      TOKEN_PROGRAM_ID.toBuffer(),
      lpMintAddress.toBuffer(),
    ],
    ASSOCIATED_PROGRAM_ID
  );

  const [observationAddress] = await getOrcleAccountAddress(
    poolAddress,
    program.programId
  );

  const creatorToken0 = getAssociatedTokenAddressSync(
    token0,
    creator.publicKey,
    false,
    token0Program
  );
  const creatorToken1 = getAssociatedTokenAddressSync(
    token1,
    creator.publicKey,
    false,
    token1Program
  );

  // Derive compressed addresses

  const poolCompressedAddress = deriveAddressV2(
    poolAddress.toBytes(),
    addressTreeInfo.tree.toBytes(),
    program.programId.toBytes()
  );

  const observationCompressedAddress = deriveAddressV2(
    observationAddress.toBytes(),
    addressTreeInfo.tree.toBytes(),
    program.programId.toBytes()
  );

  // Get validity proof for new compressed addresses
  const proofRpcResult = await rpc.getValidityProofV0(
    [],
    [
      {
        tree: addressTreeInfo.tree,
        queue: addressTreeInfo.queue,
        address: bn(poolCompressedAddress),
      },
      {
        tree: addressTreeInfo.tree,
        queue: addressTreeInfo.queue,
        address: bn(observationCompressedAddress),
      },
    ]
  );

  // Set up packed accounts for compression
  const systemAccountConfig = SystemAccountMetaConfig.new(program.programId);
  const remainingAccounts =
    PackedAccounts.newWithSystemAccounts(systemAccountConfig);

  // Insert tree accounts
  const outputMerkleTreeIndex = remainingAccounts.insertOrGet(
    stateTreeInfo.queue
  );
  const addressMerkleTreePubkeyIndex = remainingAccounts.insertOrGet(
    addressTreeInfo.tree
  );
  const addressQueuePubkeyIndex = remainingAccounts.insertOrGet(
    addressTreeInfo.queue
  );

  // Create packed address tree info for both addresses
  const poolAddressTreeInfo = {
    rootIndex: proofRpcResult.rootIndices[0],
    addressMerkleTreePubkeyIndex,
    addressQueuePubkeyIndex,
  };

  const observationAddressTreeInfo = {
    rootIndex: proofRpcResult.rootIndices[1],
    addressMerkleTreePubkeyIndex,
    addressQueuePubkeyIndex,
  };

  // Create compression params
  // 229 Bytes +1
  const compressionParams = {
    poolCompressedAddress: Array.from(poolCompressedAddress),
    poolAddressTreeInfo,
    observationCompressedAddress: Array.from(observationCompressedAddress),
    observationAddressTreeInfo,
    proof: { 0: proofRpcResult.compressedProof },
    outputStateTreeIndex: outputMerkleTreeIndex,
  };

  // Get compression config account
  const compressionConfigKey = PublicKey.findProgramAddressSync(
    [
      Buffer.from([
        99, 111, 109, 112, 114, 101, 115, 115, 105, 98, 108, 101, 95, 99, 111,
        110, 102, 105, 103,
      ]),
      Buffer.from([0]),
    ],
    program.programId
  )[0];

  const {
    remainingAccounts: systemAccountMetas,
    systemStart,
    packedStart,
  } = remainingAccounts.toAccountMetas();

  const initializeIx = await program.methods
    .initialize(
      initAmount.initAmount0,
      initAmount.initAmount1,
      new BN(0),
      compressionParams
    )
    .accountsStrict({
      creator: creator.publicKey,
      ammConfig: configAddress,
      authority: auth,
      poolState: poolAddress,
      token0Mint: token0,
      token1Mint: token1,
      lpMint: lpMintAddress,
      creatorToken0,
      creatorToken1,
      creatorLpToken: creatorLpTokenAddress,
      token0Vault: vault0,
      token1Vault: vault1,
      createPoolFee,
      observationState: observationAddress,
      tokenProgram: TOKEN_PROGRAM_ID,
      token0Program: token0Program,
      token1Program: token1Program,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
      rent: SYSVAR_RENT_PUBKEY,
      config: compressionConfigKey,
      rentRecipient: creator.publicKey,
    })
    .remainingAccounts(systemAccountMetas)
    .instruction()
    .catch((e) => {
      console.log("error: ", e);
      throw e;
    });

  const lookupTableAccountPubkey = new PublicKey(
    "9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ"
  );
  const lookupTableAccount = (
    await rpc.getAddressLookupTable(lookupTableAccountPubkey, {
      commitment: "confirmed",
    })
  ).value;

  const messageV0 = new TransactionMessage({
    payerKey: creator.publicKey,
    recentBlockhash: (await program.provider.connection.getLatestBlockhash())
      .blockhash,
    instructions: [
      ComputeBudgetProgram.setComputeUnitLimit({ units: 1_200_000 }),
      initializeIx,
    ],
  }).compileToV0Message([lookupTableAccount]);

  const versionedTx = new web3.VersionedTransaction(messageV0);
  versionedTx.sign([creator]);

  const txId = await sendAndConfirmTx(rpc, versionedTx, {
    skipPreflight: false,
    commitment: "confirmed",
  }).catch(async (e: SendTransactionError) => {
    console.log("error: ", e);
    console.log("getLogs: ", await e.getLogs(program.provider.connection));
    throw e;
  });
  console.log("initialize txid: ", txId);

  const poolState = await program.account.poolState.fetch(poolAddress);
  return { poolAddress, poolState };
}

export async function deposit(
  program: Program<RaydiumCpSwap>,
  owner: Signer,
  configAddress: PublicKey,
  token0: PublicKey,
  token0Program: PublicKey,
  token1: PublicKey,
  token1Program: PublicKey,
  lp_token_amount: BN,
  maximum_token_0_amount: BN,
  maximum_token_1_amount: BN,
  confirmOptions?: ConfirmOptions
) {
  const [auth] = await getAuthAddress(program.programId);
  const [poolAddress] = await getPoolAddress(
    configAddress,
    token0,
    token1,
    program.programId
  );

  const [lpMintAddress] = await getPoolLpMintAddress(
    poolAddress,
    program.programId
  );
  const [vault0] = await getPoolVaultAddress(
    poolAddress,
    token0,
    program.programId
  );
  const [vault1] = await getPoolVaultAddress(
    poolAddress,
    token1,
    program.programId
  );
  const [ownerLpToken] = await PublicKey.findProgramAddress(
    [
      owner.publicKey.toBuffer(),
      TOKEN_PROGRAM_ID.toBuffer(),
      lpMintAddress.toBuffer(),
    ],
    ASSOCIATED_PROGRAM_ID
  );

  const onwerToken0 = getAssociatedTokenAddressSync(
    token0,
    owner.publicKey,
    false,
    token0Program
  );
  const onwerToken1 = getAssociatedTokenAddressSync(
    token1,
    owner.publicKey,
    false,
    token1Program
  );

  const tx = await program.methods
    .deposit(lp_token_amount, maximum_token_0_amount, maximum_token_1_amount)
    .accountsStrict({
      owner: owner.publicKey,
      authority: auth,
      poolState: poolAddress,
      ownerLpToken,
      token0Account: onwerToken0,
      token1Account: onwerToken1,
      token0Vault: vault0,
      token1Vault: vault1,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenProgram2022: TOKEN_2022_PROGRAM_ID,
      vault0Mint: token0,
      vault1Mint: token1,
      lpMint: lpMintAddress,
    })
    .rpc(confirmOptions);
  return tx;
}

export async function withdraw(
  program: Program<RaydiumCpSwap>,
  owner: Signer,
  configAddress: PublicKey,
  token0: PublicKey,
  token0Program: PublicKey,
  token1: PublicKey,
  token1Program: PublicKey,
  lp_token_amount: BN,
  minimum_token_0_amount: BN,
  minimum_token_1_amount: BN,
  confirmOptions?: ConfirmOptions
) {
  const [auth] = await getAuthAddress(program.programId);
  const [poolAddress] = await getPoolAddress(
    configAddress,
    token0,
    token1,
    program.programId
  );

  const [lpMintAddress] = await getPoolLpMintAddress(
    poolAddress,
    program.programId
  );
  const [vault0] = await getPoolVaultAddress(
    poolAddress,
    token0,
    program.programId
  );
  const [vault1] = await getPoolVaultAddress(
    poolAddress,
    token1,
    program.programId
  );
  const [ownerLpToken] = await PublicKey.findProgramAddress(
    [
      owner.publicKey.toBuffer(),
      TOKEN_PROGRAM_ID.toBuffer(),
      lpMintAddress.toBuffer(),
    ],
    ASSOCIATED_PROGRAM_ID
  );

  const onwerToken0 = getAssociatedTokenAddressSync(
    token0,
    owner.publicKey,
    false,
    token0Program
  );
  const onwerToken1 = getAssociatedTokenAddressSync(
    token1,
    owner.publicKey,
    false,
    token1Program
  );

  const tx = await program.methods
    .withdraw(lp_token_amount, minimum_token_0_amount, minimum_token_1_amount)
    .accounts({
      owner: owner.publicKey,
      authority: auth,
      poolState: poolAddress,
      ownerLpToken,
      token0Account: onwerToken0,
      token1Account: onwerToken1,
      token0Vault: vault0,
      token1Vault: vault1,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenProgram2022: TOKEN_2022_PROGRAM_ID,
      vault0Mint: token0,
      vault1Mint: token1,
      lpMint: lpMintAddress,
      memoProgram: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
    })
    .rpc(confirmOptions)
    .catch();

  return tx;
}

export async function swap_base_input(
  program: Program<RaydiumCpSwap>,
  owner: Signer,
  configAddress: PublicKey,
  inputToken: PublicKey,
  inputTokenProgram: PublicKey,
  outputToken: PublicKey,
  outputTokenProgram: PublicKey,
  amount_in: BN,
  minimum_amount_out: BN,
  confirmOptions?: ConfirmOptions
) {
  const [auth] = await getAuthAddress(program.programId);
  const [poolAddress] = await getPoolAddress(
    configAddress,
    inputToken,
    outputToken,
    program.programId
  );

  const [inputVault] = await getPoolVaultAddress(
    poolAddress,
    inputToken,
    program.programId
  );
  const [outputVault] = await getPoolVaultAddress(
    poolAddress,
    outputToken,
    program.programId
  );

  const inputTokenAccount = getAssociatedTokenAddressSync(
    inputToken,
    owner.publicKey,
    false,
    inputTokenProgram
  );
  const outputTokenAccount = getAssociatedTokenAddressSync(
    outputToken,
    owner.publicKey,
    false,
    outputTokenProgram
  );
  const [observationAddress] = await getOrcleAccountAddress(
    poolAddress,
    program.programId
  );
  const observationState = await program.account.observationState.fetch(
    observationAddress
  );

  const ix = await program.methods
    .swapBaseInput(amount_in, minimum_amount_out)
    .accounts({
      payer: owner.publicKey,
      authority: auth,
      ammConfig: configAddress,
      poolState: poolAddress,
      inputTokenAccount,
      outputTokenAccount,
      inputVault,
      outputVault,
      inputTokenProgram: inputTokenProgram,
      outputTokenProgram: outputTokenProgram,
      inputTokenMint: inputToken,
      outputTokenMint: outputToken,
      observationState: observationAddress,
    })
    .instruction();
  const tx = await sendTransaction(
    program.provider.connection,
    [ix],
    [owner],
    confirmOptions
  );
  return tx;
}

export async function swap_base_output(
  program: Program<RaydiumCpSwap>,
  owner: Signer,
  configAddress: PublicKey,
  inputToken: PublicKey,
  inputTokenProgram: PublicKey,
  outputToken: PublicKey,
  outputTokenProgram: PublicKey,
  amount_out_less_fee: BN,
  max_amount_in: BN,
  confirmOptions?: ConfirmOptions
) {
  const [auth] = await getAuthAddress(program.programId);
  const [poolAddress] = await getPoolAddress(
    configAddress,
    inputToken,
    outputToken,
    program.programId
  );

  const [inputVault] = await getPoolVaultAddress(
    poolAddress,
    inputToken,
    program.programId
  );
  const [outputVault] = await getPoolVaultAddress(
    poolAddress,
    outputToken,
    program.programId
  );

  const inputTokenAccount = getAssociatedTokenAddressSync(
    inputToken,
    owner.publicKey,
    false,
    inputTokenProgram
  );
  const outputTokenAccount = getAssociatedTokenAddressSync(
    outputToken,
    owner.publicKey,
    false,
    outputTokenProgram
  );
  const [observationAddress] = await getOrcleAccountAddress(
    poolAddress,
    program.programId
  );

  const tx = await program.methods
    .swapBaseOutput(max_amount_in, amount_out_less_fee)
    .accounts({
      payer: owner.publicKey,
      authority: auth,
      ammConfig: configAddress,
      poolState: poolAddress,
      inputTokenAccount,
      outputTokenAccount,
      inputVault,
      outputVault,
      inputTokenProgram: inputTokenProgram,
      outputTokenProgram: outputTokenProgram,
      inputTokenMint: inputToken,
      outputTokenMint: outputToken,
      observationState: observationAddress,
    })
    .rpc(confirmOptions);

  return tx;
}
