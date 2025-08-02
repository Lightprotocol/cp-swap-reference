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
  AccountMeta,
  TransactionMessage,
  SendTransactionError,
  ComputeBudgetProgram,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID,
  getAssociatedTokenAddressSync,
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
  defaultStaticAccountsStruct,
  LightSystemProgram,
  bn,
  TreeType,
  getDefaultAddressTreeInfo,
  TreeInfo,
  sendAndConfirmTx,
  featureFlags,
  VERSION,
} from "@lightprotocol/stateless.js";
import { keccak_256 } from "@noble/hashes/sha3";
featureFlags.version = VERSION.V2;
console.log("Version:", featureFlags.version); // 'V2'
console.log("Is V2:", featureFlags.isV2()); // true

import { createTokenProgramLookupTable } from "@lightprotocol/compressed-token";

import { ASSOCIATED_PROGRAM_ID } from "@coral-xyz/anchor/dist/cjs/utils/token";

// TODO: move to stateless.js
function deriveAddress(
  seed: Uint8Array,
  merkleTreePubkey: Uint8Array,
  programIdBytes: Uint8Array
): Uint8Array {
  // Create slices array matching the Rust implementation
  const slices = [seed, merkleTreePubkey, programIdBytes];

  // Call hashvToBn254FieldSizeBe which mirrors the Rust
  // hashv_to_bn254_field_size_be_const_array
  return fixedHashV(slices);
}

export function fixedHashV(bytes: Uint8Array[]): Uint8Array {
  const HASH_TO_FIELD_SIZE_SEED = 255; // u8::MAX

  const hasher = keccak_256.create();

  // Hash all input bytes
  for (const input of bytes) {
    hasher.update(input);
  }

  // Add the bump seed (just like Rust version)
  hasher.update(new Uint8Array([HASH_TO_FIELD_SIZE_SEED]));

  const hash = hasher.digest();

  // Truncate to BN254 field size (just like Rust version)
  hash[0] = 0;

  return hash;
}

export async function setupInitializeTest(
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
  console.log(
    "owner, payer: ",
    owner.publicKey.toBase58(),
    program.provider.wallet.payer?.publicKey.toBase58()
  );
  await initializeCompressionConfig(
    program,
    connection,
    owner,
    program.provider.wallet.payer
  );
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

  const tx = await sendTransaction(connection, [ix], [owner], confirmOptions);
  console.log("init amm config tx: ", tx);
  return address;
}

export async function initializeCompressionConfig(
  program: Program<RaydiumCpSwap>,
  connection: Connection,
  payer: Signer,
  authority: Signer,
  compressionDelay: number = 100,
  rentRecipient: PublicKey = new PublicKey(
    "CLEuMG7pzJX9xAuKCFzBP154uiG1GaNo4Fq7x6KAcAfG"
  ),
  addressSpace: PublicKey[] = [
    new PublicKey("EzKE84aVTkCUhDHLELqyJaq1Y7UVVmqxXqZjVHwHY3rK"),
  ],
  confirmOptions?: ConfirmOptions
): Promise<string> {
  // Derive compression config PDA
  const [configAddress] = PublicKey.findProgramAddressSync(
    [Buffer.from("compressible_config"), Buffer.from([0])],
    program.programId
  );

  // Get program account first to find program data account address
  const programAccount = await connection.getAccountInfo(program.programId);
  if (!programAccount) {
    throw new Error("Program account does not exist");
  }

  // For BPF Upgradeable Loader programs, the program data address is at bytes 4-35
  const programDataAddress = new PublicKey(programAccount.data.slice(4, 36));

  console.log("Program data address:", programDataAddress.toBase58());

  const programDataAccount = await connection.getAccountInfo(
    programDataAddress
  );
  if (!programDataAccount) {
    throw new Error("Program data account does not exist");
  }

  const data = programDataAccount.data;

  // Check discriminator (should be 3 for ProgramData)
  const discriminator = data.readUInt32LE(0);
  if (discriminator !== 3) {
    throw new Error("Invalid program data discriminator");
  }

  // Check if authority exists
  const hasAuthority = data[12] === 1;
  if (!hasAuthority) {
    throw new Error("Program has no upgrade authority");
  }

  // Extract upgrade authority (bytes 13-44)
  console.log("data: ", data.length, Array.from(data.slice(0, 160)));
  const authorityBytes = data.slice(13, 45);
  const upgradeAuthority = new PublicKey(authorityBytes);

  console.log("Upgrade authority:", upgradeAuthority.toBase58());

  const ix = await program.methods
    .initializeCompressionConfig(
      compressionDelay,
      rentRecipient,
      addressSpace,
      null // configBump is None/null
    )
    .accounts({
      payer: payer.publicKey,
      config: configAddress,
      programData: programDataAddress,
      authority: authority.publicKey,
      systemProgram: SystemProgram.programId,
    })
    .instruction();

  const tx = await sendTransaction(
    connection,
    [ix],
    [payer, authority],
    confirmOptions
  );
  console.log("initialize compression config tx: ", tx);
  return tx;
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

  const poolCompressedAddress = deriveAddress(
    poolAddress.toBytes(),
    addressTreeInfo.tree.toBytes(),
    program.programId.toBytes()
  );
  console.log("pool compressed address: ", Array.from(poolCompressedAddress));
  console.log("pool address: ", Array.from(poolAddress.toBytes()));
  console.log("pool address tree info: ", addressTreeInfo.tree.toBase58());
  console.log("program id: ", program.programId.toBase58());

  const observationCompressedAddress = deriveAddress(
    observationAddress.toBytes(),
    addressTreeInfo.tree.toBytes(),
    program.programId.toBytes()
  );

  console.log(
    "observation compressed address: ",
    Array.from(observationCompressedAddress)
  );
  console.log(
    "observation address: ",
    Array.from(observationAddress.toBytes())
  );
  console.log(
    "observation address tree info: ",
    addressTreeInfo.tree.toBase58()
  );
  console.log("program id: ", program.programId.toBase58());

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

  console.log(
    "roots: ",
    proofRpcResult.roots.map((r) => r.toString())
  );
  console.log(
    "roots: ",
    proofRpcResult.roots.map((r) => r.toArray())
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

  console.log("CLIENT outputMerkleTreeIndex: ", outputMerkleTreeIndex);
  console.log("CLIENT poolAddressTreeInfo: ", poolAddressTreeInfo);
  console.log(
    "CLIENT observationAddressTreeInfo: ",
    observationAddressTreeInfo
  );

  console.log("pool c address: ", Array.from(poolCompressedAddress));
  console.log(
    "observation c address: ",
    Array.from(observationCompressedAddress)
  );
  console.log("proof bytes a: ", Array.from(proofRpcResult.compressedProof.a));
  console.log("proof bytes b: ", Array.from(proofRpcResult.compressedProof.b));
  console.log("proof bytes c: ", Array.from(proofRpcResult.compressedProof.c));

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

  console.log("CLIENT systemAccountMetas (actual): ", systemAccountMetas);
  console.log("CLIENT systemAccountMetas length: ", systemAccountMetas.length);
  console.log("CLIENT systemStart: ", systemStart);
  console.log("CLIENT packedStart: ", packedStart);

  const initializeIx = await program.methods
    .initialize(
      initAmount.initAmount0,
      initAmount.initAmount1,
      new BN(0),
      compressionParams
    )
    .accounts({
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
      associatedTokenProgram: undefined,
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

  console.log("CLIENT remainingAccounts: ", remainingAccounts);
  console.log("client: poolAddress: ", poolAddress.toBase58());

  const lookupTableAccount = (
    await rpc.getAddressLookupTable(
      new PublicKey("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")
    )
  ).value!;
  const messageV0 = new TransactionMessage({
    payerKey: creator.publicKey,
    recentBlockhash: (await program.provider.connection.getLatestBlockhash())
      .blockhash,
    instructions: [
      ComputeBudgetProgram.setComputeUnitLimit({ units: 1_200_000 }),
      initializeIx,
    ],
  }).compileToV0Message([lookupTableAccount]);

  console.log(
    "init ix size serialized: ",
    messageV0.compiledInstructions.length
  );

  const versionedTx = new web3.VersionedTransaction(messageV0);
  await program.provider.wallet.signTransaction(versionedTx);
  console.log("signed tx size serialized: ", versionedTx.serialize().length);

  const tx = await sendAndConfirmTx(rpc, versionedTx).catch(
    async (e: SendTransactionError) => {
      console.log("error: ", e);
      console.log("getLogs: ", await e.getLogs(program.provider.connection));
      throw e;
    }
  );
  console.log("tx confirmed: ", tx);

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
  console.log("observationAddress: ", observationAddress);
  const observationState = await program.account.observationState.fetch(
    observationAddress
  );
  console.log("observationState: ", observationState);

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

// ZK Compression Helper Classes

class PackedAccounts {
  private preAccounts: AccountMeta[] = [];
  private systemAccounts: AccountMeta[] = [];
  private nextIndex: number = 0;
  private map: Map<string, [number, AccountMeta]> = new Map();

  static newWithSystemAccounts(
    config: SystemAccountMetaConfig
  ): PackedAccounts {
    const instance = new PackedAccounts();
    instance.addSystemAccounts(config);
    return instance;
  }

  addPreAccountsSigner(pubkey: PublicKey): void {
    this.preAccounts.push({ pubkey, isSigner: true, isWritable: false });
  }

  addPreAccountsSignerMut(pubkey: PublicKey): void {
    this.preAccounts.push({ pubkey, isSigner: true, isWritable: true });
  }

  addPreAccountsMeta(accountMeta: AccountMeta): void {
    this.preAccounts.push(accountMeta);
  }

  addSystemAccounts(config: SystemAccountMetaConfig): void {
    this.systemAccounts.push(...getLightSystemAccountMetas(config));
  }

  insertOrGet(pubkey: PublicKey): number {
    return this.insertOrGetConfig(pubkey, false, true);
  }

  insertOrGetReadOnly(pubkey: PublicKey): number {
    return this.insertOrGetConfig(pubkey, false, false);
  }

  insertOrGetConfig(
    pubkey: PublicKey,
    isSigner: boolean,
    isWritable: boolean
  ): number {
    const key = pubkey.toString();
    const entry = this.map.get(key);
    if (entry) {
      return entry[0];
    }
    const index = this.nextIndex++;
    const meta: AccountMeta = { pubkey, isSigner, isWritable };
    this.map.set(key, [index, meta]);
    return index;
  }

  private hashSetAccountsToMetas(): AccountMeta[] {
    const entries = Array.from(this.map.entries());
    entries.sort((a, b) => a[1][0] - b[1][0]);
    return entries.map(([, [, meta]]) => meta);
  }

  private getOffsets(): [number, number] {
    const systemStart = this.preAccounts.length;
    const packedStart = systemStart + this.systemAccounts.length;
    return [systemStart, packedStart];
  }

  toAccountMetas(): {
    remainingAccounts: AccountMeta[];
    systemStart: number;
    packedStart: number;
  } {
    const packed = this.hashSetAccountsToMetas();
    const [systemStart, packedStart] = this.getOffsets();
    return {
      remainingAccounts: [
        ...this.preAccounts,
        ...this.systemAccounts,
        ...packed,
      ],
      systemStart,
      packedStart,
    };
  }
}

class SystemAccountMetaConfig {
  selfProgram: PublicKey;
  cpiContext?: PublicKey;
  solCompressionRecipient?: PublicKey;
  solPoolPda?: PublicKey;

  private constructor(
    selfProgram: PublicKey,
    cpiContext?: PublicKey,
    solCompressionRecipient?: PublicKey,
    solPoolPda?: PublicKey
  ) {
    this.selfProgram = selfProgram;
    this.cpiContext = cpiContext;
    this.solCompressionRecipient = solCompressionRecipient;
    this.solPoolPda = solPoolPda;
  }

  static new(selfProgram: PublicKey): SystemAccountMetaConfig {
    return new SystemAccountMetaConfig(selfProgram);
  }

  static newWithCpiContext(
    selfProgram: PublicKey,
    cpiContext: PublicKey
  ): SystemAccountMetaConfig {
    return new SystemAccountMetaConfig(selfProgram, cpiContext);
  }
}

function getLightSystemAccountMetas(
  config: SystemAccountMetaConfig
): AccountMeta[] {
  let signerSeed = new TextEncoder().encode("cpi_authority");
  const cpiSigner = PublicKey.findProgramAddressSync(
    [signerSeed],
    config.selfProgram
  )[0];
  const defaults = SystemAccountPubkeys.default();
  const metas: AccountMeta[] = [
    { pubkey: defaults.lightSystemProgram, isSigner: false, isWritable: false },
    { pubkey: cpiSigner, isSigner: false, isWritable: false },
    {
      pubkey: defaults.registeredProgramPda,
      isSigner: false,
      isWritable: false,
    },
    { pubkey: defaults.noopProgram, isSigner: false, isWritable: false },
    {
      pubkey: defaults.accountCompressionAuthority,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: defaults.accountCompressionProgram,
      isSigner: false,
      isWritable: false,
    },
    { pubkey: config.selfProgram, isSigner: false, isWritable: false },
  ];
  if (config.solPoolPda) {
    metas.push({
      pubkey: config.solPoolPda,
      isSigner: false,
      isWritable: true,
    });
  }
  if (config.solCompressionRecipient) {
    metas.push({
      pubkey: config.solCompressionRecipient,
      isSigner: false,
      isWritable: true,
    });
  }
  metas.push({
    pubkey: defaults.systemProgram,
    isSigner: false,
    isWritable: false,
  });
  if (config.cpiContext) {
    metas.push({
      pubkey: config.cpiContext,
      isSigner: false,
      isWritable: true,
    });
  }
  return metas;
}

class SystemAccountPubkeys {
  lightSystemProgram: PublicKey;
  systemProgram: PublicKey;
  accountCompressionProgram: PublicKey;
  accountCompressionAuthority: PublicKey;
  registeredProgramPda: PublicKey;
  noopProgram: PublicKey;
  solPoolPda: PublicKey;

  private constructor(
    lightSystemProgram: PublicKey,
    systemProgram: PublicKey,
    accountCompressionProgram: PublicKey,
    accountCompressionAuthority: PublicKey,
    registeredProgramPda: PublicKey,
    noopProgram: PublicKey,
    solPoolPda: PublicKey
  ) {
    this.lightSystemProgram = lightSystemProgram;
    this.systemProgram = systemProgram;
    this.accountCompressionProgram = accountCompressionProgram;
    this.accountCompressionAuthority = accountCompressionAuthority;
    this.registeredProgramPda = registeredProgramPda;
    this.noopProgram = noopProgram;
    this.solPoolPda = solPoolPda;
  }

  static default(): SystemAccountPubkeys {
    return new SystemAccountPubkeys(
      LightSystemProgram.programId,
      SystemProgram.programId,
      defaultStaticAccountsStruct().accountCompressionProgram,
      defaultStaticAccountsStruct().accountCompressionAuthority,
      defaultStaticAccountsStruct().registeredProgramPda,
      defaultStaticAccountsStruct().noopProgram,
      PublicKey.default
    );
  }
}

// Save lookup table account state to JSON file for automatic upload in tests
export async function saveLookupTableAccountToJson(
  connection: Connection,
  lookupTableAddress: PublicKey,
  filename: string
): Promise<void> {
  const fs = require("fs");
  const path = require("path");

  console.log(
    `Fetching lookup table account data for ${lookupTableAddress.toBase58()}`
  );

  const accountInfo = await connection.getAccountInfo(lookupTableAddress);
  if (!accountInfo) {
    throw new Error(
      `Lookup table account ${lookupTableAddress.toBase58()} not found`
    );
  }

  // Format account data for JSON upload (matches solana account format)
  const accountData = {
    account: {
      data: [Buffer.from(accountInfo.data).toString("base64"), "base64"],
      executable: accountInfo.executable,
      lamports: accountInfo.lamports,
      owner: accountInfo.owner.toBase58(),
      rentEpoch: accountInfo.rentEpoch,
    },
    pubkey: lookupTableAddress.toBase58(),
  };

  // Save to file
  const filePath = path.join(process.cwd(), filename);
  fs.writeFileSync(filePath, JSON.stringify(accountData, null, 2));

  console.log(`Lookup table account saved to ${filePath}`);
  console.log(`Account address: ${lookupTableAddress.toBase58()}`);
  console.log(`Data size: ${accountInfo.data.length} bytes`);
}

// Create and save lookup table for testing
export async function createAndSaveLookupTable(
  rpc: any,
  creator: web3.Keypair,
  token0: PublicKey,
  token1: PublicKey,
  additionalAccounts: PublicKey[],
  filename: string = "test-lookup-table.json"
): Promise<PublicKey> {
  console.log("Creating lookup table...");

  const { address: lut } = await createTokenProgramLookupTable(
    rpc,
    creator,
    creator,
    [token0, token1],
    additionalAccounts
  );

  console.log("Lookup table created, saving to JSON...");

  // Save the account data to JSON
  await saveLookupTableAccountToJson(rpc, lut, filename);

  return lut;
}
