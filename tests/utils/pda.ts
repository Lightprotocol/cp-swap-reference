import * as anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { TreeInfo, BN } from "@lightprotocol/stateless.js";
import {
  deriveCompressedMintAddress,
  findMintAddress,
} from "@lightprotocol/compressed-token";

export const AMM_CONFIG_SEED = Buffer.from(
  anchor.utils.bytes.utf8.encode("amm_config")
);
export const POOL_SEED = Buffer.from(anchor.utils.bytes.utf8.encode("pool"));
export const POOL_VAULT_SEED = Buffer.from(
  anchor.utils.bytes.utf8.encode("pool_vault")
);
export const POOL_AUTH_SEED = Buffer.from(
  anchor.utils.bytes.utf8.encode("vault_and_lp_mint_auth_seed")
);
export const POOL_LPMINT_SEED = Buffer.from(
  anchor.utils.bytes.utf8.encode("pool_lp_mint")
);
export const TICK_ARRAY_SEED = Buffer.from(
  anchor.utils.bytes.utf8.encode("tick_array")
);

export const OPERATION_SEED = Buffer.from(
  anchor.utils.bytes.utf8.encode("operation")
);

export const ORACLE_SEED = Buffer.from(
  anchor.utils.bytes.utf8.encode("observation")
);

export function u16ToBytes(num: number) {
  const arr = new ArrayBuffer(2);
  const view = new DataView(arr);
  view.setUint16(0, num, false);
  return new Uint8Array(arr);
}

export function i16ToBytes(num: number) {
  const arr = new ArrayBuffer(2);
  const view = new DataView(arr);
  view.setInt16(0, num, false);
  return new Uint8Array(arr);
}

export function u32ToBytes(num: number) {
  const arr = new ArrayBuffer(4);
  const view = new DataView(arr);
  view.setUint32(0, num, false);
  return new Uint8Array(arr);
}

export function i32ToBytes(num: number) {
  const arr = new ArrayBuffer(4);
  const view = new DataView(arr);
  view.setInt32(0, num, false);
  return new Uint8Array(arr);
}

export async function getAmmConfigAddress(
  index: number,
  programId: PublicKey
): Promise<[PublicKey, number]> {
  const [address, bump] = await PublicKey.findProgramAddress(
    [AMM_CONFIG_SEED, u16ToBytes(index)],
    programId
  );
  return [address, bump];
}

export async function getAuthAddress(
  programId: PublicKey
): Promise<[PublicKey, number]> {
  const [address, bump] = await PublicKey.findProgramAddress(
    [POOL_AUTH_SEED],
    programId
  );
  return [address, bump];
}

export async function getPoolAddress(
  ammConfig: PublicKey,
  tokenMint0: PublicKey,
  tokenMint1: PublicKey,
  programId: PublicKey
): Promise<[PublicKey, number]> {
  const [address, bump] = await PublicKey.findProgramAddress(
    [
      POOL_SEED,
      ammConfig.toBuffer(),
      tokenMint0.toBuffer(),
      tokenMint1.toBuffer(),
    ],
    programId
  );

  return [address, bump];
}

export function getPoolSignerSeeds(
  ammConfig: PublicKey,
  tokenMint0: PublicKey,
  tokenMint1: PublicKey,
  programId: PublicKey
): Buffer[] {
  const seeds = [
    POOL_SEED,
    ammConfig.toBuffer(),
    tokenMint0.toBuffer(),
    tokenMint1.toBuffer(),
  ];
  const [_, bump] = PublicKey.findProgramAddressSync(seeds, programId);
  return Array.from(seeds).concat([Buffer.from([bump])]);
}

export async function getPoolVaultAddress(
  pool: PublicKey,
  vaultTokenMint: PublicKey,
  programId: PublicKey
): Promise<[PublicKey, number]> {
  const [address, bump] = await PublicKey.findProgramAddress(
    [POOL_VAULT_SEED, pool.toBuffer(), vaultTokenMint.toBuffer()],
    programId
  );
  return [address, bump];
}

export async function getPoolVaultSignerSeeds(
  pool: PublicKey,
  vaultTokenMint: PublicKey,
  programId: PublicKey
): Promise<Buffer[]> {
  const seeds = [POOL_AUTH_SEED];
  // const seeds = [POOL_VAULT_SEED, pool.toBuffer(), vaultTokenMint.toBuffer()];
  const [_, bump] = PublicKey.findProgramAddressSync(seeds, programId);
  return seeds.concat([Buffer.from([bump])]);
}

export async function getLpVaultAddress(
  lpMint: PublicKey,
  programId: PublicKey
): Promise<[PublicKey, number]> {
  const [address, bump] = await PublicKey.findProgramAddress(
    [POOL_VAULT_SEED, lpMint.toBuffer()],
    programId
  );
  return [address, bump];
}

// pda used to derive lp_mint and its compressed address.
export function getPoolLpMintSignerAddress(
  pool: PublicKey,
  programId: PublicKey
): [PublicKey, number] {
  const [address, bump] = PublicKey.findProgramAddressSync(
    [POOL_LPMINT_SEED, pool.toBuffer()],
    programId
  );
  return [address, bump];
}

export async function getPoolLpMintAddress(
  mintSignerAddress: PublicKey
): Promise<[PublicKey, number]> {
  return findMintAddress(mintSignerAddress);
}

export async function getOracleAccountAddress(
  pool: PublicKey,
  programId: PublicKey
): Promise<[PublicKey, number]> {
  const [address, bump] = await PublicKey.findProgramAddress(
    [ORACLE_SEED, pool.toBuffer()],
    programId
  );

  return [address, bump];
}

export function getOracleSignerSeeds(
  pool: PublicKey,
  programId: PublicKey
): Buffer[] {
  const seeds = [ORACLE_SEED, pool.toBuffer()];
  const [_, bump] = PublicKey.findProgramAddressSync(seeds, programId);
  return Array.from(seeds).concat([Buffer.from([bump])]);
}

/**
 * Derives the compressed mint address from the mint seed and address tree.
 * @param mintSeed The mint seed public key.
 * @param addressTreePubkey The address tree public key.
 * @returns Buffer (32 bytes) compressed mint address.
 */

export function getPoolLpMintCompressedAddress(
  mintSigner: PublicKey,
  addressTreeInfo: TreeInfo
): number[] {
  return deriveCompressedMintAddress(mintSigner, addressTreeInfo);
}

export function deriveTokenProgramConfig(
  version?: number
): [PublicKey, number] {
  const versionValue = version ?? 1;
  const registryProgramId = new PublicKey(
    "Lighton6oQpVkeewmo2mcPTQQp7kYHr4fWpAgJyEmDX"
  );

  const [compressibleConfig, configBump] = PublicKey.findProgramAddressSync(
    [
      Buffer.from("compressible_config"),
      new BN(versionValue).toArrayLike(Buffer, "le", 8),
    ],
    registryProgramId
  );

  return [compressibleConfig, configBump];
}
