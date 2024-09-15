import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { createMint, getOrCreateAssociatedTokenAccount, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { LAMPORTS_PER_SOL } from "@solana/web3.js";
import { assert } from "chai";
import { configDotenv } from "dotenv";
import fs from "fs";
import { LotterySystem } from "../target/types/lottery_system";

configDotenv();

describe("lottery_system", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.LotterySystem as Program<LotterySystem>;

  const authorityData = JSON.parse(fs.readFileSync("./authority.json", 'utf8'));
  const user1Data = JSON.parse(fs.readFileSync("./user1.json", 'utf8'));
  const user2Data = JSON.parse(fs.readFileSync("./user2.json", 'utf8'));

  const authority = anchor.web3.Keypair.fromSecretKey(new Uint8Array(authorityData));
  const user1 = anchor.web3.Keypair.fromSecretKey(new Uint8Array(user1Data));
  const user2 = anchor.web3.Keypair.fromSecretKey(new Uint8Array(user2Data));

  let mint: anchor.web3.PublicKey;
  let vaultTokenAccount: anchor.web3.PublicKey;
  let user1TokenAccount: anchor.web3.PublicKey;
  let user2TokenAccount: anchor.web3.PublicKey;

  before(async () => {
    await Promise.all([
      provider.connection.confirmTransaction(
        await provider.connection.requestAirdrop(authority.publicKey, 1 * LAMPORTS_PER_SOL), "finalized"
      ),
      provider.connection.confirmTransaction(
        await provider.connection.requestAirdrop(user1.publicKey, 1 * LAMPORTS_PER_SOL), "finalized"
      ),
      provider.connection.confirmTransaction(
        await provider.connection.requestAirdrop(user2.publicKey, 1 * LAMPORTS_PER_SOL), "finalized"
      ),
    ]);
    console.log(`authority ${authority.publicKey} has a balance of ${(await provider.connection.getBalance(authority.publicKey)) / LAMPORTS_PER_SOL}`);
    console.log(`user1 ${user1.publicKey} has a balance of ${(await provider.connection.getBalance(user1.publicKey)) / LAMPORTS_PER_SOL}`);
    console.log(`user2 ${user2.publicKey} has a balance of ${(await provider.connection.getBalance(user2.publicKey)) / LAMPORTS_PER_SOL}`);

    // Create mint and token accounts
    mint = await createMint(provider.connection, authority, authority.publicKey, null, 6);

    const authTokenAccount = await getOrCreateAssociatedTokenAccount(provider.connection, authority, mint, authority.publicKey);
    console.log("authTokenAccount:", authTokenAccount);
    const user1TokenAccount = await getOrCreateAssociatedTokenAccount(provider.connection, user1, mint, user1.publicKey);
    console.log("user1TokenAccount:", user1TokenAccount);
    const user2TokenAccount = await getOrCreateAssociatedTokenAccount(provider.connection, user2, mint, user2.publicKey);
    console.log("user2TokenAccount:", user2TokenAccount);

    // await mintTo(provider.connection, authority, mint, authority.publicKey, authority.publicKey, 1000);
    // await mintTo(provider.connection, authority, mint, user1.publicKey, authority.publicKey, 1000);
    // await mintTo(provider.connection, authority, mint, user2.publicKey, authority.publicKey, 1000);

    // const authTokenBalance = await provider.connection.getTokenAccountBalance(authority.publicKey);
    // console.log("authTokenBalance:", authTokenBalance.value.amount);
    // const user1TokenBalance = await provider.connection.getTokenAccountBalance(user1.publicKey);
    // console.log("user1TokenBalance:", user1TokenBalance.value.amount);
    // const user2TokenBalance = await provider.connection.getTokenAccountBalance(user2.publicKey);
    // console.log("user2TokenBalance:", user2TokenBalance.value.amount);
  });

  it("Initializes creator pool", async () => {
    const poolId = new anchor.BN(1);
    const totalTickets = new anchor.BN(1000);

    const creatorPool = anchor.web3.Keypair.generate();

    // airdrop SOL to creatorPool
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(creatorPool.publicKey, 1 * LAMPORTS_PER_SOL), "finalized"
    );
    console.log(`creatorPool ${creatorPool.publicKey} has a balance of ${(await provider.connection.getBalance(creatorPool.publicKey)) / LAMPORTS_PER_SOL}`);

    try {
      console.log("initializing creator pool");
      const tx = await program.methods.initializeCreatorPool(poolId, totalTickets).accounts({
        creatorPool: creatorPool.publicKey,
        authority: authority.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      }).signers([creatorPool, authority]).transaction();

      tx.feePayer = authority.publicKey; // Set fee payer
      tx.recentBlockhash = (await provider.connection.getRecentBlockhash()).blockhash;

      const txSig = await provider.connection.sendTransaction(tx, [creatorPool, authority]);
      await provider.connection.confirmTransaction(txSig, "finalized");
      console.log("txSig:", txSig);
    } catch (error) {
      console.log(error);
    }

    const poolAccount = await program.account.creatorPool.fetch(creatorPool.publicKey);
    console.log("poolAccount:", poolAccount);
    assert.ok(poolAccount.poolId.eq(poolId));
    assert.ok(poolAccount.totalTickets.eq(totalTickets));
  });

  it("Accumulates tickets for users", async () => {
    const user1Tickets = anchor.web3.Keypair.generate();
    const user2Tickets = anchor.web3.Keypair.generate();

    const creatorPool = new anchor.web3.PublicKey(
      "CyuJVnLCaGg7sUyAuGKL4LaT1dmcKDa4LydumLc85ig6"
    );

    const buyer = anchor.web3.Keypair.generate();
    const vault = anchor.web3.Keypair.generate();

    const tx = await program.methods
      .accumulateTickets()
      .accounts({
        creatorPool: creatorPool,
        buyer: user1.publicKey,
        buyerTokenAccount: buyer.publicKey,
        vaultTokenAccount: vault.publicKey,
        userTickets: user1Tickets.publicKey,
        userNfts: user1.publicKey, // Assuming user1 has no NFTs for simplicity
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([user1, user1Tickets])
      .transaction();

    tx.feePayer = authority.publicKey; // Set fee payer
    tx.recentBlockhash = (await provider.connection.getRecentBlockhash()).blockhash;
    const txSig = await provider.connection.sendTransaction(tx, [user1, user1Tickets]);
    await provider.connection.confirmTransaction(txSig, "finalized");
    console.log("txSig:", txSig);

    await program.methods
      .accumulateTickets()
      .accounts({
        creatorPool: creatorPool,
        buyer: user2.publicKey,
        buyerTokenAccount: user2TokenAccount,
        vaultTokenAccount: vaultTokenAccount,
        userTickets: user2Tickets.publicKey,
        userNfts: user2.publicKey, // Assuming user2 has no NFTs for simplicity
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([user2, user2Tickets])
      .transaction();

    const user1TicketAccount = await program.account.userTickets.fetch(user1Tickets.publicKey);
    const user2TicketAccount = await program.account.userTickets.fetch(user2Tickets.publicKey);

    assert.ok(user1TicketAccount.balance.gt(new anchor.BN(0)));
    assert.ok(user2TicketAccount.balance.gt(new anchor.BN(0)));
  });

  it("Draws lottery", async () => {
    // We need to wait for at least 4 hours before drawing the lottery
    // For testing purposes, we can use a provider.connection.simulateBlockhash to simulate time passing
    // await provider.connection.simulateBlockhash(await provider.connection.getLatestBlockhash());

    const creatorPool = new anchor.web3.PublicKey(
      "CyuJVnLCaGg7sUyAuGKL4LaT1dmcKDa4LydumLc85ig6"
    );

    await program.methods
      .drawLottery()
      .accounts({
        creatorPool: creatorPool,
        winner: user1.publicKey, // Assuming user1 wins for this test
        winnerTokenAccount: user1TokenAccount,
        vaultTokenAccount: vaultTokenAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .transaction();

    assert.ok(user1.publicKey.equals(user1.publicKey));
    assert.ok(user1TokenAccount.equals(user1TokenAccount));
    assert.ok(vaultTokenAccount.equals(vaultTokenAccount));
  });

  it("Sells tickets", async () => {
    const user1Tickets = anchor.web3.Keypair.generate();
    // Assuming user1 has tickets to sell

    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(user1Tickets.publicKey, 1 * LAMPORTS_PER_SOL), "finalized"
    );
    await program.methods
      .sellTickets(new anchor.BN(100))
      .accounts({
        vaultTokenAccount: vaultTokenAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
        userTickets: user1Tickets.publicKey,
        userTokenAccount: user1TokenAccount,
      })
      .signers([user1])
      .transaction();
  });

  it("Voids expired tickets", async () => {
    // Assuming user1 has expired tickets

    const user1Tickets = anchor.web3.Keypair.generate();

    await program.methods
      .voidExpiredTickets()
      .accounts({
        userTickets: user1Tickets.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([user1])
      .transaction();
  });

  it("Distributes penalty", async () => {
    const user1Tickets = anchor.web3.Keypair.generate();

    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(user1Tickets.publicKey, 1 * LAMPORTS_PER_SOL), "finalized"
    );

    // Assuming user1 has tickets to sell
    await program.methods
      .distributePenalty()
      .accounts({
        userTickets: user1Tickets.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .transaction();
  });
});
