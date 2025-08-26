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
} from "@solana/web3.js";
import { Rpc, TreeInfo, MerkleContext } from "@lightprotocol/stateless.js";

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
  // Fetches account info irrespective of whether it's currently compressed or
  // decompressed.
  const info = await rpc.getCompressibleAccountInfo(
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
