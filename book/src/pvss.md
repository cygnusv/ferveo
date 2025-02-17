# Publicly Verifiable Secret Sharing

The PVSS scheme used is a modified Scrape PVSS. 

## Dealer's role

1. The dealer chooses a uniformly random polynomial \\(f(x) = \sum^p_i a_i x^i \\) of degree \\(t\\).
2. Let \\(F_0, \ldots, F_p \leftarrow [a_0] G_1, \ldots, [a_t] G_1 \\)
3. Let \\(\hat{u}_2 \rightarrow [a_0] \hat{u_1} \\)
4. For each validator \\(i\\), for each \\(\omega_j \in \Omega_i\\), encrypt the evaluation \\( \hat{Y}_{i, \omega_j} \leftarrow [f(\omega_j)] ek_i  \\)
4. Post the signed message \\(\tau, (F_0, \ldots, F_t), \hat{u}\_2, (\hat{Y}\_{i,\omega_j})\\) to the blockchain

## Public verification

1. Check \\(e(F_0, \hat{u}_1)=  e(G_1, \hat{u_2})\\)
2. Compute by FFT \\(A_1, \ldots, A_W \leftarrow [f(\omega_0)]G_1, \ldots, [f(\omega_W)]G_1 \\)
3. Partition \\(A_1, \ldots, A_W\\) into \\(A_{i,\omega_j} \\) for validator \\(i\\)'s shares \\(\omega_j\\)
4. For each encrypted share \\(\hat{Y}\_{i,\omega_i} \\), check \\(e(G_1, \hat{Y}\_{i,\omega_j}) = e(A_{i,\omega_j}, ek_i) \\)

## Aggregation

Two PVSS instances \\( (\{F_j\}, \hat{u}\_2, \hat{Y}\_{i,\omega_j}) \\) may be aggregated into a single PVSS instance by adding elementwise each of the corresponding group elements.

## Validator decryption of private key shares

A validator \\(i\\) recovers their private key shares \\(Z_{i,\omega_j}\\) from the shares encrypted to their public encryption key \\(ek_i\\):

\\[Z_{i, \omega_j} = [dk_i^{-1}] \hat{Y}_{i, \omega_j} \\]

## Public Aggregation

Multiple PVSS instances can be aggregated into one by a single validator, speeding up verification time. The aggregation and verification are similar to the Aggregatable DKG paper.

## Consensus

It is critical that all validators agree on which PVSS instances are used to create the final key; in particular, this is exactly what makes Ferveo depend on a synchronous consensus protocol like Tendermint. Therefore, the validators must all verify the PVSS instances and agree on the set of valid PVSS instances; or in the case where a validator has aggregated all PVSS instances, the validator set must agree on a valid aggregation of PVSS instances.

However, although full nodes can certainly perform the verification of a PVSS instance or aggregation, full nodes do not need to verify either the PVSS instances or the aggregation. 