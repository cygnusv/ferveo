# Introduction

Ferveo is a **fast** platform for distributed key generation and threshold decryption that can be integrated into Tendermint. Using Ferveo, a validator set can generate a distributed private key where each validator's share of the private key is weighted proportionally to their staked amount. The distributed key generation (DKG) is efficient enough to perform once per epoch and the underlying cryptographic schemes allow for real-time decryption of encrypted transactions. Ferveo allows the validator set to commit to encrypted transactions prior to threshold decryption, preventing public visibility of pending transactions and helping prevent transaction front-running.