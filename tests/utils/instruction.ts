import { Program, IdlTypes, BN } from "@coral-xyz/anchor";
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
  fetchAccountInterface,
  getLpVaultAddress,
  getPoolLpMintSignerAddress,
  getPoolLpMintCompressedAddress,
  getPoolSignerSeeds,
  getOracleSignerSeeds,
  getPoolVaultSignerSeeds,
  deriveTokenProgramConfig,
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
  buildAndSignTx,
  createPackedAccountsSmallWithCpiContext,
  packCompressAccountsIdempotent,
  packDecompressAccountsIdempotent,
  CTOKEN_PROGRAM_ID,
} from "@lightprotocol/stateless.js";

import {
  CompressedTokenProgram,
  CTOKEN_RENT_SPONSOR,
  getAccountInterface,
  getAssociatedCTokenAddressAndBump,
  getAtaProgramId,
  buildDecompressParams,
} from "@lightprotocol/compressed-token";

featureFlags.version = VERSION.V2;
const COMPRESSION_DELAY = 0;
const ADDRESS_SPACE = [
  new PublicKey("EzKE84aVTkCUhDHLELqyJaq1Y7UVVmqxXqZjVHwHY3rK"),
];

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
    console.log("initializeCompressionConfig signature:", txId);
  }

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

  const [address, _] = deriveCompressionConfigAddress(program.programId);
  if (!(await accountExist(connection, address))) {
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

  await compressHelper(
    program,
    owner,
    poolState.ammConfig,
    poolState.token0Mint,
    poolState.token1Mint,
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
  const rpc = createRpc();

  const addressTreeInfo = getDefaultAddressTreeInfo();
  const stateTreeInfo = selectStateTreeInfo(await rpc.getStateTreeInfos());

  const [auth, _authBump] = await getAuthAddress(program.programId);

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
    token0Program,
    getAtaProgramId(token0Program)
  );

  const creatorToken1 = getAssociatedTokenAddressSync(
    token1,
    creator.publicKey,
    false,
    token1Program,
    getAtaProgramId(token1Program)
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
    name: Buffer.from("Test Token"),
    symbol: Buffer.from("TTT"),
    uri: Buffer.from("https://test.com"),
  };
  // Get compression config PDA
  const [compressionConfig] = deriveCompressionConfigAddress(program.programId);

  const packedAccountMetas = remainingAccounts.toAccountMetas();

  const [lpVault] = await getLpVaultAddress(lpMintAddress, program.programId);

  // Get ctoken config PDA
  const [ctokenConfigAccount] = deriveTokenProgramConfig();

  const ctokenRentSponsor = CTOKEN_RENT_SPONSOR;

  console.log("creatorToken0", creatorToken0.toBase58());
  console.log("creatorToken1", creatorToken1.toBase58());
  console.log("creatorLpToken", creatorLpToken.toBase58());
  console.log("lpVault", lpVault.toBase58());
  console.log("token0Vault", vault0.toBase58());
  console.log("token1Vault", vault1.toBase58());

  console.log("token0", token0.toBase58());
  console.log("token1", token1.toBase58());
  console.log("lpMintAddress", lpMintAddress.toBase58());

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
      ctokenConfigAccount,
      ctokenRentRecipient: ctokenRentSponsor,
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
  const txId = await sendAndConfirmTx(rpc, tx, { skipPreflight: true });
  console.log("initialize signature:", txId);

  const { parsed: poolState } = await fetchAccountInterface(
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

// Compressed all program accounts idempotently.
export async function compressIdempotent(
  program: Program<RaydiumCpSwap>,
  feePayer: Signer,
  poolAddress: PublicKey,
  observationAddress: PublicKey,
  lpVault: PublicKey,
  token0Vault: PublicKey,
  token1Vault: PublicKey,
  signerSeeds: Buffer<ArrayBufferLike>[][],
  rpc: Rpc,
  confirmOptions?: ConfirmOptions,
  compressionAuthority?: PublicKey,
  tokenCompressionAuthority?: PublicKey,
  rentRecipient?: PublicKey
) {
  compressionAuthority = compressionAuthority ?? feePayer.publicKey;
  tokenCompressionAuthority = tokenCompressionAuthority ?? feePayer.publicKey;
  rentRecipient = rentRecipient ?? feePayer.publicKey;

  const addressTreeInfo = getDefaultAddressTreeInfo();
  const stateTreeInfo = selectStateTreeInfo(await rpc.getStateTreeInfos());

  const {
    accountInfo: poolAccountInfo,
    parsed: poolState,
    merkleContext: poolMerkleContext,
  } = await fetchAccountInterface(
    poolAddress,
    addressTreeInfo,
    program,
    "poolState",
    rpc
  );

  const {
    accountInfo: observationAccountInfo,
    parsed: observationState,
    merkleContext: observationMerkleContext,
  } = await fetchAccountInterface(
    observationAddress,
    addressTreeInfo,
    program,
    "observationState",
    rpc
  );
  const { accountInfo: lpVaultAccountInfo, parsed: lpVaultState } =
    await getAccountInterface(
      rpc,
      lpVault,
      undefined,
      CompressedTokenProgram.programId
    );

  const { accountInfo: token0VaultAccountInfo, parsed: token0VaultState } =
    await getAccountInterface(
      rpc,
      token0Vault,
      undefined,
      CompressedTokenProgram.programId
    );
  const { accountInfo: token1VaultAccountInfo, parsed: token1VaultState } =
    await getAccountInterface(
      rpc,
      token1Vault,
      undefined,
      CompressedTokenProgram.programId
    );

  if (
    !poolMerkleContext &&
    !observationMerkleContext &&
    !token0VaultState &&
    !token1VaultState
  )
    return;

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

  const {
    compressedAccountMetas,
    systemAccountsOffset,
    remainingAccounts,
    proofOption,
  } = await packCompressAccountsIdempotent(
    program.programId,
    proof,
    [
      {
        accountId: poolAddress,
        accountInfo: poolAccountInfo,
        parsed: poolState,
      },
      {
        accountId: observationAddress,
        accountInfo: observationAccountInfo,
        parsed: observationState,
      },
      {
        accountId: lpVault,
        accountInfo: lpVaultAccountInfo,
        parsed: lpVaultState,
      },
      {
        accountId: token0Vault,
        accountInfo: token0VaultAccountInfo,
        parsed: token0VaultState,
      },
      {
        accountId: token1Vault,
        accountInfo: token1VaultAccountInfo,
        parsed: token1VaultState,
      },
    ],
    stateTreeInfo
  );

  const [config] = deriveCompressionConfigAddress(program.programId);

  // FIXME: proofOption is received onchain as some even if it is passed ass
  // none.
  const compressIx = await program.methods
    .compressAccountsIdempotent(
      { 0: proof.compressedProof },
      compressedAccountMetas,
      signerSeeds,
      systemAccountsOffset
    )
    .accountsStrict({
      feePayer: feePayer.publicKey,
      config,
      rentRecipient,
      compressionAuthority,
      ctokenCompressionAuthority: tokenCompressionAuthority,
      ctokenRentSponsor: CTOKEN_RENT_SPONSOR,
      ctokenProgram: CompressedTokenProgram.programId,
      ctokenCpiAuthority: CompressedTokenProgram.deriveCpiAuthorityPda,
    })
    .remainingAccounts(remainingAccounts)
    .instruction();

  return compressIx;
}

// Decompress all program accounts idempotently. Clients should prepend this
// instruction to their txns if any of the CompressibleAccountInfos return
// isCompressed=true.
export async function decompressIdempotent(
  program: Program<RaydiumCpSwap>,
  owner: Signer,
  poolAddress: PublicKey,
  observationAddress: PublicKey,
  lpVault: PublicKey,
  token0Vault: PublicKey,
  token1Vault: PublicKey,
  configAddress: PublicKey,
  lpMint: PublicKey,
  token0: PublicKey,
  token1: PublicKey,
  rpc: Rpc
) {
  const addressTreeInfo = getDefaultAddressTreeInfo();

  const poolStateInterface = await fetchAccountInterface(
    poolAddress,
    addressTreeInfo,
    program,
    "poolState",
    rpc
  );

  const observationInterface = await fetchAccountInterface(
    observationAddress,
    addressTreeInfo,
    program,
    "observationState",
    rpc
  );

  const lpVaultInterface = await getAccountInterface(
    rpc,
    lpVault,
    undefined,
    CompressedTokenProgram.programId
  );
  const token0VaultInterface = await getAccountInterface(
    rpc,
    token0Vault,
    undefined,
    CompressedTokenProgram.programId
  );
  const token1VaultInterface = await getAccountInterface(
    rpc,
    token1Vault,
    undefined,
    CompressedTokenProgram.programId
  );

  console.log("poolStateInterface", poolStateInterface);
  console.log("observationInterface", observationInterface);
  console.log("lpVaultInterface", lpVaultInterface);
  console.log("token0VaultInterface", token0VaultInterface);
  console.log("token1VaultInterface", token1VaultInterface);

  // Use SDK helper to build decompress params
  const decompressParams = await buildDecompressParams(program.programId, rpc, [
    {
      address: poolAddress,
      info: poolStateInterface,
      accountType: "poolState",
    },
    {
      address: observationAddress,
      info: observationInterface,
      accountType: "observationState",
    },
    {
      address: lpVault,
      info: lpVaultInterface,
      accountType: "cTokenData",
      tokenVariant: "lpVault",
    },
    {
      address: token0Vault,
      info: token0VaultInterface,
      accountType: "cTokenData",
      tokenVariant: "token0Vault",
    },
    {
      address: token1Vault,
      info: token1VaultInterface,
      accountType: "cTokenData",
      tokenVariant: "token1Vault",
    },
  ]);

  // If nothing compressed, return null
  if (!decompressParams) {
    return null;
  }
  console.log(
    "REFERENCE decompressParams.systemAccountsOffset",
    decompressParams.systemAccountsOffset
  );
  console.log(
    `[REFERENCE] compressedAccounts:`,
    JSON.stringify(decompressParams.compressedAccounts, null, 2)
  );

  const [ctokenConfig] = deriveTokenProgramConfig();

  const decompressIx = await program.methods
    .decompressAccountsIdempotent(
      decompressParams.proofOption,
      decompressParams.compressedAccounts,
      decompressParams.systemAccountsOffset
    )
    .accountsStrict({
      feePayer: owner.publicKey,
      config: deriveCompressionConfigAddress(program.programId)[0],
      rentPayer: owner.publicKey,
      ctokenRentSponsor: CTOKEN_RENT_SPONSOR,
      ctokenProgram: CompressedTokenProgram.programId,
      ctokenCpiAuthority: CompressedTokenProgram.deriveCpiAuthorityPda,
      ctokenConfig,
      ammConfig: configAddress,
      token0Mint: token0,
      token1Mint: token1,
      lpMint,
      poolState: poolAddress,
    })
    .remainingAccounts(decompressParams.remainingAccounts)
    .instruction();

  return decompressIx;
}

export async function compressHelper(
  program: Program<RaydiumCpSwap>,
  owner: Signer,
  configAddress: PublicKey,
  token0: PublicKey,
  token1: PublicKey,
  confirmOptions?: ConfirmOptions
) {
  const rpc = createRpc();
  const [auth] = await getAuthAddress(program.programId);
  const [poolAddress, poolBump] = await getPoolAddress(
    configAddress,
    token0,
    token1,
    program.programId
  );
  const [observationAddress, observationBump] = await getOracleAccountAddress(
    poolAddress,
    program.programId
  );
  const poolSignerSeeds = getPoolSignerSeeds(
    configAddress,
    token0,
    token1,
    program.programId
  );
  const observationSignerSeeds = getOracleSignerSeeds(
    poolAddress,
    program.programId
  );

  const [mintSigner] = getPoolLpMintSignerAddress(
    poolAddress,
    program.programId
  );
  const [lpMintAddress] = await getPoolLpMintAddress(mintSigner);
  const [lpVault] = await getLpVaultAddress(lpMintAddress, program.programId);
  const lpVaultSignerSeeds = await getPoolVaultSignerSeeds(
    poolAddress,
    token0,
    program.programId
  );

  const [token0VaultAddress] = await getPoolVaultAddress(
    poolAddress,
    token0,
    program.programId
  );
  const token0VaultSignerSeeds = await getPoolVaultSignerSeeds(
    poolAddress,
    token0,
    program.programId
  );

  const [token1VaultAddress] = await getPoolVaultAddress(
    poolAddress,
    token1,
    program.programId
  );
  const token1VaultSignerSeeds = await getPoolVaultSignerSeeds(
    poolAddress,
    token1,
    program.programId
  );

  const signerSeeds = [
    poolSignerSeeds,
    observationSignerSeeds,
    lpVaultSignerSeeds,
    token0VaultSignerSeeds,
    token1VaultSignerSeeds,
  ];

  const compressIx = await compressIdempotent(
    program,
    owner,
    poolAddress,
    observationAddress,
    lpVault,
    token0VaultAddress,
    token1VaultAddress,
    signerSeeds,
    rpc,
    confirmOptions,
    undefined,
    undefined,
    undefined
  );
  const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
    units: 1_200_000,
  });
  const { blockhash } = await program.provider.connection.getLatestBlockhash();
  const { value: lookupTableAccount } = await rpc.getAddressLookupTable(
    new PublicKey("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")
  );

  const compressTx = buildAndSignTx(
    [computeBudgetIx, compressIx],
    owner,
    blockhash,
    [],
    [lookupTableAccount]
  );
  const compressTxId = await sendAndConfirmTx(rpc, compressTx, confirmOptions);

  return compressTxId;
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

  const ownerToken0 = getAssociatedTokenAddressSync(
    token0,
    owner.publicKey,
    false,
    token0Program,
    getAtaProgramId(token0Program)
  );
  const ownerToken1 = getAssociatedTokenAddressSync(
    token1,
    owner.publicKey,
    false,
    token1Program,
    getAtaProgramId(token1Program)
  );

  // Fetch observation address
  const [observationAddress, observationBump] = await getOracleAccountAddress(
    poolAddress,
    program.programId
  );

  // Decompress accounts
  const decompressIx = await decompressIdempotent(
    program,
    owner,
    poolAddress,
    observationAddress,
    lpVaultAddress,
    vault0,
    vault1,
    configAddress,
    lpMintAddress,
    token0,
    token1,
    rpc
  );

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
    [computeBudgetIx, decompressIx, depositIx],
    owner,
    blockhash,
    [],
    [lookupTableAccount]
  );
  const depositTxId = await sendAndConfirmTx(rpc, depositTx, confirmOptions);
  console.log("decompress + deposit signature:", depositTxId);
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
    token0Program,
    getAtaProgramId(token0Program)
  );
  const ownerToken1 = getAssociatedTokenAddressSync(
    token1,
    owner.publicKey,
    false,
    token1Program,
    getAtaProgramId(token1Program)
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
  const withdrawTxId = await sendAndConfirmTx(rpc, withdrawTx, confirmOptions);
  console.log("withdraw signature:", withdrawTxId);
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

  const [lpMintSignerAddress] = getPoolLpMintSignerAddress(
    poolAddress,
    program.programId
  );
  const [lpMintAddress] = await getPoolLpMintAddress(lpMintSignerAddress);
  const [lpVaultAddress] = await getLpVaultAddress(
    lpMintAddress,
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
    inputTokenProgram,
    getAtaProgramId(inputTokenProgram)
  );
  const outputTokenAccount = getAssociatedTokenAddressSync(
    outputToken,
    owner.publicKey,
    false,
    outputTokenProgram,
    getAtaProgramId(outputTokenProgram)
  );
  const [observationAddress] = await getOracleAccountAddress(
    poolAddress,
    program.programId
  );

  const decompressIx = await decompressIdempotent(
    program,
    owner,
    poolAddress,
    observationAddress,
    lpVaultAddress,
    inputVault,
    outputVault,
    configAddress,
    lpMintAddress,
    inputToken,
    outputToken,
    createRpc()
  );

  console.log(
    "decompressIx compare:",
    decompressIx.keys.map(
      (k, idx) =>
        "#" +
        (idx + 1) +
        " " +
        k.pubkey.toString() +
        "   writable:" +
        k.isWritable +
        "   signer:" +
        k.isSigner
    )
  );

  const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
    units: 1_200_000,
  });

  console.log(
    "compressible configAddress:",
    deriveCompressionConfigAddress(program.programId)[0].toString()
  );

  // With auto-resolution: constants and defaults are automatically filled if not provided
  // You can now omit: ctokenProgram, ctokenCpiAuthority (constants)
  // and: config, ctokenRentSponsor, ctokenConfig (defaults)
  const tx = await program.methods
    .swapBaseInput(amount_in, minimum_amount_out)
    .preInstructions([computeBudgetIx])
    .decompressIfNeeded({
      feePayer: owner.publicKey,
      rentPayer: owner.publicKey,

      // Seed accounts for compressible accounts being decompressed.
      // required if names do not match name in main instruction.
      // ammconfig is part of main ixn threfore doesnt have to be provided here
      // ammConfig: configAddress,
      token0Mint: inputToken,
      token1Mint: outputToken,

      // compressible accounts we want to decompress if needed
      poolState: poolAddress,
      observationState: observationAddress,
      token0Vault: inputVault,
      token1Vault: outputVault,
    })
    .accountsStrict({
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
    .transaction();

  console.log(" len of ixs:", tx.instructions.length);

  const rpc = createRpc();
  const { blockhash } = await program.provider.connection.getLatestBlockhash();
  const { value: lookupTableAccount } = await rpc.getAddressLookupTable(
    new PublicKey("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")
  );

  const versionedTx = buildAndSignTx(
    tx.instructions,
    owner,
    blockhash,
    [],
    [lookupTableAccount]
  );
  const txId = await sendAndConfirmTx(rpc, versionedTx, confirmOptions);
  console.log("swap base_in signature:", txId);
  return txId;
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

  const [lpMintSignerAddress] = getPoolLpMintSignerAddress(
    poolAddress,
    program.programId
  );
  const [lpMintAddress] = await getPoolLpMintAddress(lpMintSignerAddress);
  const [lpVaultAddress] = await getLpVaultAddress(
    lpMintAddress,
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
    inputTokenProgram,
    getAtaProgramId(inputTokenProgram)
  );
  const outputTokenAccount = getAssociatedTokenAddressSync(
    outputToken,
    owner.publicKey,
    false,
    outputTokenProgram,
    getAtaProgramId(outputTokenProgram)
  );
  const [observationAddress] = await getOracleAccountAddress(
    poolAddress,
    program.programId
  );

  const decompressIx = await decompressIdempotent(
    program,
    owner,
    poolAddress,
    observationAddress,
    lpVaultAddress,
    inputVault,
    outputVault,
    configAddress,
    lpMintAddress,
    inputToken,
    outputToken,
    createRpc()
  );

  const ix = await program.methods
    .swapBaseOutput(max_amount_in, amount_out_less_fee)
    .accountsStrict({
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
    .instruction();

  const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
    units: 1_200_000,
  });
  const rpc = createRpc();
  const { blockhash } = await program.provider.connection.getLatestBlockhash();
  const { value: lookupTableAccount } = await rpc.getAddressLookupTable(
    new PublicKey("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")
  );

  const tx = buildAndSignTx(
    [computeBudgetIx, decompressIx, ix],
    owner,
    blockhash,
    [],
    [lookupTableAccount]
  );
  const txId = await sendAndConfirmTx(rpc, tx, confirmOptions);
  console.log("swap base_out signature:", txId);

  return txId;
}
