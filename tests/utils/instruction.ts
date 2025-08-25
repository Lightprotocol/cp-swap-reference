import { Program, BN, IdlTypes } from "@coral-xyz/anchor";
import { RaydiumCpSwap } from "../../target/types/raydium_cp_swap";
import {
  Connection,
  ConfirmOptions,
  PublicKey,
  Keypair,
  Signer,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  ComputeBudgetProgram,
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
  getOracleAccountAddress,
  fetchCompressibleAccount,
  POOL_SEED,
  ORACLE_SEED,
  getLpVaultAddress,
  getPoolLpMintSignerAddress,
  getPoolLpMintCompressedAddress,
} from "./index";
import {
  createRpc,
  bn,
  sendAndConfirmTx,
  featureFlags,
  VERSION,
  Rpc,
  deriveAddressV2,
  initializeCompressionConfig,
  selectStateTreeInfo,
  getDefaultAddressTreeInfo,
  packTreeInfos,
  deriveCompressionConfigAddress,
  createPackedAccountsSmall,
  buildAndSignTx,
  PackedStateTreeInfo,
  createPackedAccountsSmallWithCpiContext,
} from "@lightprotocol/stateless.js";

import {
  CompressedTokenProgram,
  createTokenPool,
  getAssociatedCTokenAddressAndBump,
} from "@lightprotocol/compressed-token";
import { bs58 } from "@coral-xyz/anchor/dist/cjs/utils/bytes";

featureFlags.version = VERSION.V2;
const COMPRESSION_DELAY = 100;
const ADDRESS_SPACE = [
  new PublicKey("EzKE84aVTkCUhDHLELqyJaq1Y7UVVmqxXqZjVHwHY3rK"),
];

type CompressedAccountVariant =
  IdlTypes<RaydiumCpSwap>["compressedAccountVariant"];

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

  const [address, _] = deriveCompressionConfigAddress(program.programId);
  if (!(await accountExist(connection, address))) {
    await initializeCompressionConfig(
      connection,
      owner,
      program.programId,
      program.provider.wallet.payer,
      COMPRESSION_DELAY,
      program.provider.wallet.payer.publicKey,
      ADDRESS_SPACE
    );
  }
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

  const [address, _] = deriveCompressionConfigAddress(program.programId);
  if (!(await accountExist(connection, address))) {
    // Extend connection with zkcompression endpoints
    const rpc = createRpc();
    const txId = await initializeCompressionConfig(
      rpc,
      owner,
      program.programId,
      program.provider.wallet.payer,
      COMPRESSION_DELAY,
      program.provider.wallet.payer.publicKey,
      ADDRESS_SPACE
    );
    console.log("initializeCompressionConfig txId", txId);
  }

  while (1) {
    const [{ token0, token0Program }, { token1, token1Program }] =
      await createTokenMintAndAssociatedTokenAccount(
        connection,
        owner,
        new Keypair(),
        transferFeeConfig
      );

    console.log("Created tokens:", token0.toString(), token1.toString());

    if (tokenProgramRequired != undefined) {
      if (
        token0Program.equals(tokenProgramRequired.token0Program) &&
        token1Program.equals(tokenProgramRequired.token1Program)
      ) {
        console.log("Found matching programs, using these tokens");
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

  const [address, _] = deriveCompressionConfigAddress(program.programId);
  if (!(await accountExist(connection, address))) {
    // Extend connection with zkcompression endpoints
    const rpc = createRpc();
    await initializeCompressionConfig(
      rpc,
      owner,
      program.programId,
      program.provider.wallet.payer,
      COMPRESSION_DELAY,
      program.provider.wallet.payer.publicKey,
      ADDRESS_SPACE
    );
  }

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
  // Extend connection with zkcompression endpoints
  const rpc = createRpc();

  const addressTreeInfo = getDefaultAddressTreeInfo();
  const stateTreeInfo = selectStateTreeInfo(await rpc.getStateTreeInfos());

  const [auth, authBump] = await getAuthAddress(program.programId);

  const [poolAddress] = await getPoolAddress(
    configAddress,
    token0,
    token1,
    program.programId
  );

  // 1. mintSigner
  const [lpMintSignerAddress] = getPoolLpMintSignerAddress(
    poolAddress,
    program.programId
  );
  // 2. lpMint
  const [lpMintAddress, lpMintBump] = await getPoolLpMintAddress(
    lpMintSignerAddress
  );

  // 3. cMint
  const lpMintCompressedAddress = getPoolLpMintCompressedAddress(
    lpMintSignerAddress,
    addressTreeInfo
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

  const [observationAddress] = await getOracleAccountAddress(
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

  // 1. Derive compressed addresses
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

  // Get validity proof
  // Must match the ordering used by the program when invoking the cpi.
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
      {
        tree: addressTreeInfo.tree,
        queue: addressTreeInfo.queue,
        address: bn(lpMintCompressedAddress),
      },
    ]
  );

  // Set up compression-related accounts
  const remainingAccounts = createPackedAccountsSmallWithCpiContext(
    program.programId,
    stateTreeInfo.cpiContext
  );
  // adds state tree and address tree
  const outputStateTreeIndex = remainingAccounts.insertOrGet(
    stateTreeInfo.queue
  );
  const packedTreeInfos = packTreeInfos(proofRpcResult, remainingAccounts);

  const [creatorLpToken, creatorLpTokenBump] =
    getAssociatedCTokenAddressAndBump(creator.publicKey, lpMintAddress);

  // Create compression-related ix data
  // 229 Bytes +1
  const compressionParams = {
    // poolstate
    poolAddressTreeInfo: packedTreeInfos.addressTrees[0],
    // observation
    observationAddressTreeInfo: packedTreeInfos.addressTrees[1],
    // mint
    lpMintAddressTreeInfo: packedTreeInfos.addressTrees[2],
    lpMintBump,
    // shared
    proof: { 0: proofRpcResult.compressedProof },
    outputStateTreeIndex,
    creatorLpTokenBump,
  };
  // Get compression config PDA
  const [compressionConfig] = deriveCompressionConfigAddress(program.programId);

  const packedAccountMetas = remainingAccounts.toAccountMetas();
  console.log("REM packedAccountMetas: ", remainingAccounts.getNamedMetas());
  console.log("REM packed rem: ", packedAccountMetas.remainingAccounts);

  const [lpVault] = await getLpVaultAddress(lpMintAddress, program.programId);

  const ct0pId = await createTokenPool(
    rpc,
    creator,
    token0,
    confirmOptions,
    token0Program
  );
  console.log("token0: ", token0.toString());
  console.log("token0Program: ", token0Program.toString());
  console.log("ct0pId: ", ct0pId.toString());

  const ct1pId = await createTokenPool(
    rpc,
    creator,
    token1,
    confirmOptions,
    token1Program
  );
  console.log("token1: ", token1.toString());
  console.log("token1Program: ", token1Program.toString());
  console.log("ct1pId: ", ct1pId.toString());

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
      creatorLpToken,
      lpVault,
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
      compressionConfig,
      rentRecipient: creator.publicKey,
      lpMintSigner: lpMintSignerAddress,
      compressedTokenProgramCpiAuthority:
        CompressedTokenProgram.deriveCpiAuthorityPda,
      compressedTokenProgram: CompressedTokenProgram.programId,
      compressedToken0PoolPda:
        CompressedTokenProgram.deriveTokenPoolPda(token0),
      compressedToken1PoolPda:
        CompressedTokenProgram.deriveTokenPoolPda(token1),
    })
    .remainingAccounts(packedAccountMetas.remainingAccounts)
    .instruction();

  const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
    units: 1_200_000,
  });
  const { blockhash } = await program.provider.connection.getLatestBlockhash();
  const { value: lookupTableAccount } = await rpc.getAddressLookupTable(
    new PublicKey("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")
  );

  const tx = buildAndSignTx(
    [computeBudgetIx, initializeIx],
    creator,
    blockhash,
    [],
    [lookupTableAccount]
  );
  const txId = await sendAndConfirmTx(rpc, tx, confirmOptions);
  console.log("initialize txId", txId);

  const { account: poolState } = await fetchCompressibleAccount(
    poolAddress,
    addressTreeInfo,
    program,
    "poolState",
    rpc
  );

  if (!poolState) {
    throw new Error("Failed to fetch pool state");
  }

  return { poolAddress, poolState };
}

export async function decompressIdempotent(
  program: Program<RaydiumCpSwap>,
  owner: Signer,
  poolAddress: PublicKey,
  poolBump: number,
  observationAddress: PublicKey,
  observationBump: number,
  configAddress: PublicKey,
  token0: PublicKey,
  token1: PublicKey,
  rpc: Rpc,
  confirmOptions?: ConfirmOptions
): Promise<string> {
  const addressTreeInfo = getDefaultAddressTreeInfo();

  // Fetch pool state
  const { account: poolState, merkleContext: poolMerkleContext } =
    await fetchCompressibleAccount(
      poolAddress,
      addressTreeInfo,
      program,
      "poolState",
      rpc
    );

  // Fetch observation state
  const { account: observationState, merkleContext: observationMerkleContext } =
    await fetchCompressibleAccount(
      observationAddress,
      addressTreeInfo,
      program,
      "observationState",
      rpc
    );

  if (!poolMerkleContext && !observationMerkleContext) return;

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

  const proof = await rpc.getValidityProofV0([
    {
      hash: poolMerkleContext.hash,
      tree: poolMerkleContext.treeInfo.tree,
      queue: poolMerkleContext.treeInfo.queue,
    },
    {
      hash: observationMerkleContext.hash,
      tree: observationMerkleContext.treeInfo.tree,
      queue: observationMerkleContext.treeInfo.queue,
    },
  ]);

  // Prepare remaining accounts
  const remainingAccounts = createPackedAccountsSmall(program.programId);
  remainingAccounts.addPreAccountsMeta({
    isSigner: false,
    isWritable: true,
    pubkey: poolAddress,
  });
  remainingAccounts.addPreAccountsMeta({
    isSigner: false,
    isWritable: true,
    pubkey: observationAddress,
  });
  const packedTreeInfos = packTreeInfos(proof, remainingAccounts);

  // Prepare compressed accounts data
  const compressedAccountsData: {
    meta: {
      treeInfo: PackedStateTreeInfo;
      address: number[];
      outputStateTreeIndex: number;
    };
    data: CompressedAccountVariant;
    seeds: Buffer[];
  }[] = [
    {
      meta: {
        treeInfo: packedTreeInfos.stateTrees.packedTreeInfos[0],
        address: Array.from(poolCompressedAddress),
        outputStateTreeIndex: packedTreeInfos.stateTrees.outputTreeIndex,
      },
      data: { poolState: [poolState] },
      seeds: [
        POOL_SEED,
        configAddress.toBuffer(),
        token0.toBuffer(),
        token1.toBuffer(),
      ],
    },
    {
      meta: {
        treeInfo: packedTreeInfos.stateTrees.packedTreeInfos[1],
        address: Array.from(observationCompressedAddress),
        outputStateTreeIndex: packedTreeInfos.stateTrees.outputTreeIndex,
      },
      data: { observationState: [observationState] },
      seeds: [ORACLE_SEED, poolAddress.toBuffer()],
    },
  ];

  const decompressIx = await program.methods
    .decompressAccountsIdempotent(
      { 0: proof.compressedProof },
      compressedAccountsData,
      Buffer.from([poolBump, observationBump]),
      compressedAccountsData.length
    )
    .accountsStrict({
      feePayer: owner.publicKey,
      rentPayer: owner.publicKey,
      config: deriveCompressionConfigAddress(program.programId)[0],
    })
    .remainingAccounts(remainingAccounts.toAccountMetas().remainingAccounts)
    .instruction();

  // Build and send transaction
  const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
    units: 1_200_000,
  });
  const { blockhash } = await program.provider.connection.getLatestBlockhash();
  const { value: lookupTableAccount } = await rpc.getAddressLookupTable(
    new PublicKey("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")
  );
  const tx = buildAndSignTx(
    [computeBudgetIx, decompressIx],
    owner,
    blockhash,
    [],
    [lookupTableAccount]
  );
  const decompressTxId = await sendAndConfirmTx(rpc, tx, confirmOptions);

  return decompressTxId;
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
  // Extend connection with zkcompression endpoints
  const rpc = createRpc();
  const [auth] = await getAuthAddress(program.programId);
  const [poolAddress, poolBump] = await getPoolAddress(
    configAddress,
    token0,
    token1,
    program.programId
  );

  const [mintSigner] = getPoolLpMintSignerAddress(
    poolAddress,
    program.programId
  );
  const [lpMintAddress] = await getPoolLpMintAddress(mintSigner);
  const [lpVaultAddress] = await getLpVaultAddress(
    lpMintAddress,
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
  const ownerLpToken = getAssociatedTokenAddressSync(
    lpMintAddress,
    owner.publicKey,
    false,
    CompressedTokenProgram.programId,
    CompressedTokenProgram.programId
  );
  console.log("ownerLpToken", ownerLpToken.toString());

  const ownerToken0 = getAssociatedTokenAddressSync(
    token0,
    owner.publicKey,
    false,
    token0Program
  );
  const ownerToken1 = getAssociatedTokenAddressSync(
    token1,
    owner.publicKey,
    false,
    token1Program
  );

  // Fetch observation address
  const [observationAddress, observationBump] = await getOracleAccountAddress(
    poolAddress,
    program.programId
  );

  // Decompress accounts
  await decompressIdempotent(
    program,
    owner,
    poolAddress,
    poolBump,
    observationAddress,
    observationBump,
    configAddress,
    token0,
    token1,
    rpc,
    confirmOptions
  );

  console.log("lpVaultAddress", lpVaultAddress.toString());
  console.log("ownerLpToken", ownerLpToken.toString());
  console.log("authority", auth.toString());

  const depositIx = await program.methods
    .deposit(lp_token_amount, maximum_token_0_amount, maximum_token_1_amount)
    .accountsStrict({
      owner: owner.publicKey,
      authority: auth,
      poolState: poolAddress,
      ownerLpToken,
      token0Account: ownerToken0,
      token1Account: ownerToken1,
      token0Vault: vault0,
      token1Vault: vault1,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenProgram2022: TOKEN_2022_PROGRAM_ID,
      vault0Mint: token0,
      vault1Mint: token1,
      lpVault: lpVaultAddress,
      compressedTokenProgram: CompressedTokenProgram.programId,
      compressedTokenProgramCpiAuthority:
        CompressedTokenProgram.deriveCpiAuthorityPda,
      compressedToken0PoolPda:
        CompressedTokenProgram.deriveTokenPoolPda(token0),
      compressedToken1PoolPda:
        CompressedTokenProgram.deriveTokenPoolPda(token1),
    })
    .instruction();

  const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
    units: 1_200_000,
  });
  const { blockhash } = await program.provider.connection.getLatestBlockhash();
  const { value: lookupTableAccount } = await rpc.getAddressLookupTable(
    new PublicKey("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")
  );
  const depositTx = buildAndSignTx(
    [computeBudgetIx, depositIx],
    owner,
    blockhash,
    [],
    [lookupTableAccount]
  );
  const depositTxId = await sendAndConfirmTx(rpc, depositTx, confirmOptions);
  console.log("deposit txId", depositTxId);
  return depositTxId;
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

  const [lpMintSignerAddress] = getPoolLpMintSignerAddress(
    poolAddress,
    program.programId
  );
  const [lpMintAddress] = await getPoolLpMintAddress(lpMintSignerAddress);

  const [lpVaultAddress] = await getLpVaultAddress(
    lpMintAddress,
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
  const ownerLpToken = getAssociatedTokenAddressSync(
    lpMintAddress,
    owner.publicKey,
    false,
    CompressedTokenProgram.programId,
    CompressedTokenProgram.programId
  );

  const ownerToken0 = getAssociatedTokenAddressSync(
    token0,
    owner.publicKey,
    false,
    token0Program
  );
  const ownerToken1 = getAssociatedTokenAddressSync(
    token1,
    owner.publicKey,
    false,
    token1Program
  );

  const withdrawIx = await program.methods
    .withdraw(lp_token_amount, minimum_token_0_amount, minimum_token_1_amount)
    .accountsStrict({
      owner: owner.publicKey,
      authority: auth,
      poolState: poolAddress,
      ownerLpToken,
      token0Account: ownerToken0,
      token1Account: ownerToken1,
      token0Vault: vault0,
      token1Vault: vault1,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenProgram2022: TOKEN_2022_PROGRAM_ID,
      vault0Mint: token0,
      vault1Mint: token1,
      lpVault: lpVaultAddress,
      compressedTokenProgram: CompressedTokenProgram.programId,
      compressedTokenProgramCpiAuthority:
        CompressedTokenProgram.deriveCpiAuthorityPda,
      compressedToken0PoolPda:
        CompressedTokenProgram.deriveTokenPoolPda(token0),
      compressedToken1PoolPda:
        CompressedTokenProgram.deriveTokenPoolPda(token1),
      memoProgram: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
    })
    .instruction();

  const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
    units: 1_200_000,
  });
  const rpc = createRpc();
  const { blockhash } = await program.provider.connection.getLatestBlockhash();
  const { value: lookupTableAccount } = await rpc.getAddressLookupTable(
    new PublicKey("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")
  );
  const withdrawTx = buildAndSignTx(
    [computeBudgetIx, withdrawIx],
    owner,
    blockhash,
    [],
    [lookupTableAccount]
  );
  console.log("withdrawTx", bs58.encode(withdrawTx.signatures[0]));
  const withdrawTxId = await sendAndConfirmTx(rpc, withdrawTx, confirmOptions);
  console.log("withdrawTxId", withdrawTxId);
  return withdrawTxId;
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
  const [observationAddress] = await getOracleAccountAddress(
    poolAddress,
    program.programId
  );
  const observationState = await program.account.observationState.fetch(
    observationAddress
  );

  const ix = await program.methods
    .swapBaseInput(amount_in, minimum_amount_out)
    .accountsPartial({
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
  const [observationAddress] = await getOracleAccountAddress(
    poolAddress,
    program.programId
  );

  const tx = await program.methods
    .swapBaseOutput(max_amount_in, amount_out_less_fee)
    .accountsPartial({
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
      compressedTokenProgram: CompressedTokenProgram.programId,
      compressedTokenProgramCpiAuthority:
        CompressedTokenProgram.deriveCpiAuthorityPda,
      compressedToken0PoolPda:
        CompressedTokenProgram.deriveTokenPoolPda(inputToken),
      compressedToken1PoolPda:
        CompressedTokenProgram.deriveTokenPoolPda(outputToken),
    })
    .rpc(confirmOptions);

  return tx;
}
