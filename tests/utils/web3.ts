import * as anchor from "@coral-xyz/anchor";
import { Program, Idl, IdlAccounts } from "@coral-xyz/anchor";
import {
  Connection,
  Signer,
  Transaction,
  TransactionInstruction,
  TransactionSignature,
  ConfirmOptions,
  PublicKey,
  AccountInfo,
} from "@solana/web3.js";
import {
  bn,
  Rpc,
  deriveAddressV2,
  TreeInfo,
  MerkleContext,
} from "@lightprotocol/stateless.js";

export async function accountExist(
  connection: anchor.web3.Connection,
  account: anchor.web3.PublicKey
) {
  const info = await connection.getAccountInfo(account);
  if (info == null || info.data.length == 0) {
    return false;
  }
  return true;
}

export async function sendTransaction(
  connection: Connection,
  ixs: TransactionInstruction[],
  signers: Array<Signer>,
  options?: ConfirmOptions
): Promise<TransactionSignature> {
  const tx = new Transaction();
  for (var i = 0; i < ixs.length; i++) {
    tx.add(ixs[i]);
  }

  if (options == undefined) {
    options = {
      preflightCommitment: "confirmed",
      commitment: "confirmed",
    };
  }

  const sendOpt = options && {
    skipPreflight: options.skipPreflight,
    preflightCommitment: options.preflightCommitment || options.commitment,
  };

  tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
  const signature = await connection.sendTransaction(tx, signers, sendOpt);

  const status = (
    await connection.confirmTransaction(signature, options.commitment)
  ).value;

  if (status.err) {
    throw new Error(
      `Raw transaction ${signature} failed (${JSON.stringify(status)})`
    );
  }
  return signature;
}

export async function getBlockTimestamp(
  connection: Connection
): Promise<number> {
  let slot = await connection.getSlot();
  return await connection.getBlockTime(slot);
}

// Anchor-only
export async function fetchCompressibleAccount<
  TIdl extends Idl,
  TAccountName extends keyof IdlAccounts<TIdl>
>(
  address: PublicKey,
  addressTreeInfo: TreeInfo,
  anchorProgram: Program<TIdl>,
  accountName: TAccountName,
  rpc: Rpc
): Promise<{
  account: IdlAccounts<TIdl>[TAccountName];
  merkleContext?: MerkleContext;
} | null> {
  const info = await getCompressibleAccountInfo(
    address,
    anchorProgram.programId,
    addressTreeInfo,
    rpc
  );

  if (info) {
    const account = anchorProgram.coder.accounts.decode(
      accountName as string,
      info.accountInfo.data
    ) as IdlAccounts<TIdl>[TAccountName];
    return { account, merkleContext: info.merkleContext };
  }

  return null;
}

/**
 * Get account info from either compressed or onchain storage.
 * @param address         The account address to fetch.
 * @param programId       The owner program ID.
 * @param addressTreeInfo The address tree info used to store the account.
 * @param rpc             The RPC client to use.
 *
 * @returns               Account info with compression info, or null if account
 *                        doesn't exist.
 */
export async function getCompressibleAccountInfo(
  address: PublicKey,
  programId: PublicKey,
  addressTreeInfo: TreeInfo,
  rpc: Rpc
): Promise<{
  accountInfo: AccountInfo<Buffer>;
  merkleContext?: MerkleContext;
} | null> {
  const cAddress = deriveAddressV2(
    address.toBytes(),
    addressTreeInfo.tree.toBytes(),
    programId.toBytes()
  );

  // Execute both calls in parallel
  const [onchainResult, compressedResult] = await Promise.allSettled([
    rpc.getAccountInfo(address),
    rpc.getCompressedAccount(bn(Array.from(cAddress))),
  ]);

  const onchainAccount =
    onchainResult.status === "fulfilled" ? onchainResult.value : null;
  const compressedAccount =
    compressedResult.status === "fulfilled" ? compressedResult.value : null;

  if (onchainAccount) {
    console.log("is onchainAccount");
    return { accountInfo: onchainAccount, merkleContext: undefined };
  }

  // is compressed.
  if (compressedAccount && compressedAccount.data.data.length > 0) {
    const accountInfo: AccountInfo<Buffer> = {
      executable: false,
      owner: compressedAccount.owner, // TODO: check if this will become an issue.
      lamports: compressedAccount.lamports.toNumber(),
      data: Buffer.concat([
        Buffer.from(compressedAccount.data.discriminator),
        compressedAccount.data.data,
      ]),
    };
    return {
      accountInfo,
      merkleContext: {
        treeInfo: addressTreeInfo,
        hash: compressedAccount.hash,
        leafIndex: compressedAccount.leafIndex,
        proveByIndex: compressedAccount.proveByIndex,
      },
    };
  }

  // account does not exist.
  return null;
}

// TODO: fix.
/**
 * Helper to check if account data has compression info
 */
export function getCompressionInfo(accountData: any): {
  compressionInfo: any | null;
  isCompressed: boolean;
} {
  const compressionInfo = accountData?.compression_info || null;
  return {
    compressionInfo,
    isCompressed: compressionInfo !== null,
  };
}
