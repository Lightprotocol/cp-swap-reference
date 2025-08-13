import * as anchor from "@coral-xyz/anchor";
import { Program, Idl } from "@coral-xyz/anchor";
import {
  Connection,
  Signer,
  Transaction,
  TransactionInstruction,
  TransactionSignature,
  ConfirmOptions,
  PublicKey,
} from "@solana/web3.js";
import {
  createRpc,
  bn,
  Rpc,
  deriveAddressV2,
  getDefaultAddressTreeInfo,
  AddressTreeInfo,
  TreeInfo,
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

/**
 * Get parsed account data from compressed or onchain storage
 * ONLY accepts Rpc - if you don't have Rpc, don't use this function
 */
export async function getParsedCompressibleAccount<T = any>(
  address: PublicKey,
  addressTreeInfo: TreeInfo,
  decoder: (data: Buffer) => T,
  programId: PublicKey,
  rpc: Rpc
): Promise<T | null> {
  const promises = [];

  // Try onchain (Rpc extends Connection)
  // promises.push(
  //   rpc
  //     .getAccountInfo(address)
  //     .then((info) => (info ? decoder(info.data) : null))
  //     .catch(() => null)
  // );

  // Try compressed
  promises.push(
    (async () => {
      try {
        const compressedAddress = deriveAddressV2(
          address.toBytes(),
          addressTreeInfo.tree.toBytes(),
          programId.toBytes()
        );
        const cAccount = await rpc.getCompressedAccount(
          bn(Array.from(compressedAddress))
        );
        // Skip discriminator bytes
        return decoder(
          Buffer.concat([
            Buffer.from(cAccount.data.discriminator), // FIXME: this seems not to match anchor discriminator.
            cAccount.data.data,
          ])
        );
        // return decoder(Buffer.concat([Buffer.alloc(8, 0), cAccount.data.data]));
      } catch {
        return null;
      }
    })()
  );

  const results = await Promise.allSettled(promises);

  // Return first successful result
  for (const result of results) {
    if (result.status === "fulfilled" && result.value) {
      return result.value;
    }
  }

  return null;
}

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
