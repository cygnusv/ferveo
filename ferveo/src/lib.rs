#![warn(rust_2018_idioms)]

#[cfg(feature = "bindings-wasm")]
extern crate alloc;

#[cfg(feature = "bindings-python")]
pub mod bindings_python;

#[cfg(feature = "bindings-wasm")]
pub mod bindings_wasm;

pub mod api;
pub mod dkg;
pub mod primitives;
pub mod pvss;
pub mod refresh;
pub mod validator;

#[cfg(test)]
mod test_common;

pub use dkg::*;
pub use primitives::*;
pub use pvss::*;
pub use refresh::*;
pub use validator::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    ThresholdEncryptionError(#[from] ferveo_tdec::Error),

    /// DKG validator set must contain the validator with the given address
    #[error("Expected validator to be a part of the DKG validator set: {0}")]
    DealerNotInValidatorSet(EthereumAddress),

    /// DKG received an unknown dealer. Dealer must be the part of the DKG validator set.
    #[error("DKG received an unknown dealer: {0}")]
    UnknownDealer(EthereumAddress),

    /// DKG received a PVSS transcript from a dealer that has already been dealt.
    #[error("DKG received a PVSS transcript from a dealer that has already been dealt: {0}")]
    DuplicateDealer(EthereumAddress),

    /// DKG received an invalid transcript for which optimistic verification failed
    #[error("DKG received an invalid transcript from validator: {0}")]
    InvalidPvssTranscript(EthereumAddress),

    /// Not enough validators to perform the DKG for a given number of shares
    #[error("Not enough validators (expected {0}, got {1})")]
    InsufficientValidators(u32, u32),

    /// Transcript aggregate doesn't match the received PVSS instances
    #[error("Transcript aggregate doesn't match the received PVSS instances")]
    InvalidTranscriptAggregate,

    /// The validator public key doesn't match the one in the DKG
    #[error("Validator public key mismatch")]
    ValidatorPublicKeyMismatch,

    #[error(transparent)]
    BincodeError(#[from] bincode::Error),

    #[error(transparent)]
    ArkSerializeError(#[from] ark_serialize::SerializationError),

    /// Invalid byte length
    #[error("Invalid byte length. Expected {0}, got {1}")]
    InvalidByteLength(usize, usize),

    /// Invalid variant
    #[error("Invalid variant: {0}")]
    InvalidVariant(String),

    /// DKG parameters validaiton failed
    #[error("Invalid DKG parameters: number of shares {0}, threshold {1}")]
    InvalidDkgParameters(u32, u32),

    /// Failed to access a share for a given share index
    #[error("Invalid share index: {0}")]
    InvalidShareIndex(u32),

    /// Failed to produce a precomputed variant decryption share
    #[error("Invalid DKG parameters for precomputed variant: number of shares {0}, threshold {1}")]
    InvalidDkgParametersForPrecomputedVariant(u32, u32),

    /// DKG may not contain duplicated share indices
    #[error("Duplicated share index: {0}")]
    DuplicatedShareIndex(u32),

    /// Creating a transcript aggregate requires at least one transcript
    #[error("No transcripts to aggregate")]
    NoTranscriptsToAggregate,

    /// The number of messages may not be greater than the number of validators
    #[error("Invalid aggregate verification parameters: number of validators {0}, number of messages: {1}")]
    InvalidAggregateVerificationParameters(u32, u32),

    /// Too many transcripts received by the DKG
    #[error("Too many transcripts. Expected: {0}, got: {1}")]
    TooManyTranscripts(u32, u32),

    /// Received a duplicated transcript from a validator
    #[error("Received a duplicated transcript from validator: {0}")]
    DuplicateTranscript(EthereumAddress),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod test_dkg_full {
    use std::collections::HashMap;

    use ark_bls12_381::{Bls12_381 as E, Fr, G1Affine};
    use ark_ec::{AffineRepr, CurveGroup};
    use ark_ff::{UniformRand, Zero};
    use ark_std::test_rng;
    use ferveo_common::Keypair;
    use ferveo_tdec::{
        self, DecryptionSharePrecomputed, DecryptionShareSimple, SecretBox,
        SharedSecret,
    };
    use itertools::{izip, Itertools};
    use rand::{seq::SliceRandom, Rng};
    use test_case::test_case;

    use super::*;
    use crate::test_common::*;

    pub fn create_shared_secret_simple_tdec(
        dkg: &PubliclyVerifiableDkg<E>,
        aad: &[u8],
        ciphertext_header: &ferveo_tdec::CiphertextHeader<E>,
        validator_keypairs: &[Keypair<E>],
        transcripts: &[PubliclyVerifiableSS<E>],
    ) -> (
        AggregatedTranscript<E>,
        Vec<DecryptionShareSimple<E>>,
        SharedSecret<E>,
    ) {
        let pvss_aggregated =
            AggregatedTranscript::from_transcripts(transcripts).unwrap();
        assert!(pvss_aggregated
            .aggregate
            .verify_aggregation(dkg, transcripts)
            .unwrap());

        let decryption_shares: Vec<DecryptionShareSimple<E>> =
            validator_keypairs
                .iter()
                .map(|validator_keypair| {
                    let validator = dkg
                        .get_validator(&validator_keypair.public_key())
                        .unwrap();
                    pvss_aggregated
                        .aggregate
                        .create_decryption_share_simple(
                            ciphertext_header,
                            aad,
                            validator_keypair,
                            validator.share_index,
                        )
                        .unwrap()
                })
                // We take only the first `security_threshold` decryption shares
                .take(dkg.dkg_params.security_threshold() as usize)
                .collect();

        let domain_points = &dkg.domain_points()[..decryption_shares.len()];
        assert_eq!(domain_points.len(), decryption_shares.len());

        let lagrange_coeffs =
            ferveo_tdec::prepare_combine_simple::<E>(domain_points);
        let shared_secret = ferveo_tdec::share_combine_simple::<E>(
            &decryption_shares,
            &lagrange_coeffs,
        );
        (pvss_aggregated, decryption_shares, shared_secret)
    }

    #[test_case(4, 4; "number of shares (validators) is a power of 2")]
    #[test_case(7, 7; "number of shares (validators) is not a power of 2")]
    #[test_case(4, 6; "number of validators greater than the number of shares")]
    fn test_dkg_simple_tdec(shares_num: u32, validators_num: u32) {
        let rng = &mut test_rng();

        let security_threshold = shares_num / 2 + 1;
        let (dkg, validator_keypairs, messages) =
            setup_dealt_dkg_with_n_validators(
                security_threshold,
                shares_num,
                validators_num,
            );
        let transcripts = messages
            .iter()
            .take(shares_num as usize)
            .map(|m| m.1.clone())
            .collect::<Vec<_>>();
        let public_key = AggregatedTranscript::from_transcripts(&transcripts)
            .unwrap()
            .public_key;
        let ciphertext = ferveo_tdec::encrypt::<E>(
            SecretBox::new(MSG.to_vec()),
            AAD,
            &public_key,
            rng,
        )
        .unwrap();

        let (_, _, shared_secret) = create_shared_secret_simple_tdec(
            &dkg,
            AAD,
            &ciphertext.header().unwrap(),
            validator_keypairs.as_slice(),
            &transcripts,
        );

        let plaintext = ferveo_tdec::decrypt_with_shared_secret(
            &ciphertext,
            AAD,
            &shared_secret,
            &dkg.pvss_params.g_inv(),
        )
        .unwrap();
        assert_eq!(plaintext, MSG);
    }

    #[test_case(4, 4; "number of shares (validators) is a power of 2")]
    #[test_case(7, 7; "number of shares (validators) is not a power of 2")]
    // TODO: This test fails:
    // #[test_case(4, 6; "number of validators greater than the number of shares")]
    fn test_dkg_simple_tdec_precomputed(shares_num: u32, validators_num: u32) {
        let rng = &mut test_rng();

        // In precomputed variant, threshold must be equal to shares_num
        let security_threshold = shares_num;
        let (dkg, validator_keypairs, messages) =
            setup_dealt_dkg_with_n_validators(
                security_threshold,
                shares_num,
                validators_num,
            );
        let transcripts = messages
            .iter()
            .take(shares_num as usize)
            .map(|m| m.1.clone())
            .collect::<Vec<_>>();
        let pvss_aggregated =
            AggregatedTranscript::from_transcripts(&transcripts).unwrap();
        assert!(pvss_aggregated
            .aggregate
            .verify_aggregation(&dkg, &transcripts)
            .unwrap());
        let public_key = pvss_aggregated.public_key;
        let ciphertext = ferveo_tdec::encrypt::<E>(
            SecretBox::new(MSG.to_vec()),
            AAD,
            &public_key,
            rng,
        )
        .unwrap();

        let mut decryption_shares: Vec<DecryptionSharePrecomputed<E>> =
            validator_keypairs
                .iter()
                .map(|validator_keypair| {
                    let validator = dkg
                        .get_validator(&validator_keypair.public_key())
                        .unwrap();
                    pvss_aggregated
                        .aggregate
                        .create_decryption_share_simple_precomputed(
                            &ciphertext.header().unwrap(),
                            AAD,
                            validator_keypair,
                            validator.share_index,
                            &dkg.domain_points(),
                        )
                        .unwrap()
                })
                // We take only the first `security_threshold` decryption shares
                .take(dkg.dkg_params.security_threshold() as usize)
                .collect();

        // Order of decryption shares is not important in the precomputed variant
        decryption_shares.shuffle(rng);

        // Decrypt with precomputed variant
        let shared_secret =
            ferveo_tdec::share_combine_precomputed::<E>(&decryption_shares);
        let plaintext = ferveo_tdec::decrypt_with_shared_secret(
            &ciphertext,
            AAD,
            &shared_secret,
            &dkg.pvss_params.g_inv(),
        )
        .unwrap();
        assert_eq!(plaintext, MSG);
    }

    #[test_case(4, 4; "number of shares (validators) is a power of 2")]
    #[test_case(7, 7; "number of shares (validators) is not a power of 2")]
    #[test_case(4, 6; "number of validators greater than the number of shares")]
    fn test_dkg_simple_tdec_share_verification(
        shares_num: u32,
        validators_num: u32,
    ) {
        let rng = &mut test_rng();
        let security_threshold = shares_num / 2 + 1;

        let (dkg, validator_keypairs, messages) =
            setup_dealt_dkg_with_n_validators(
                security_threshold,
                shares_num,
                validators_num,
            );
        let transcripts = messages
            .iter()
            .take(shares_num as usize)
            .map(|m| m.1.clone())
            .collect::<Vec<_>>();
        let public_key = AggregatedTranscript::from_transcripts(&transcripts)
            .unwrap()
            .public_key;
        let ciphertext = ferveo_tdec::encrypt::<E>(
            SecretBox::new(MSG.to_vec()),
            AAD,
            &public_key,
            rng,
        )
        .unwrap();

        let (pvss_aggregated, decryption_shares, _) =
            create_shared_secret_simple_tdec(
                &dkg,
                AAD,
                &ciphertext.header().unwrap(),
                validator_keypairs.as_slice(),
                &transcripts,
            );

        izip!(
            &pvss_aggregated.aggregate.shares,
            &validator_keypairs,
            &decryption_shares,
        )
        .for_each(
            |(aggregated_share, validator_keypair, decryption_share)| {
                assert!(decryption_share.verify(
                    aggregated_share,
                    &validator_keypair.public_key().encryption_key,
                    &dkg.pvss_params.h,
                    &ciphertext,
                ));
            },
        );

        // Testing red-path decryption share verification
        let decryption_share = decryption_shares[0].clone();

        // Should fail because of the bad decryption share
        let mut with_bad_decryption_share = decryption_share.clone();
        with_bad_decryption_share.decryption_share = TargetField::zero();
        assert!(!with_bad_decryption_share.verify(
            &pvss_aggregated.aggregate.shares[0],
            &validator_keypairs[0].public_key().encryption_key,
            &dkg.pvss_params.h,
            &ciphertext,
        ));

        // Should fail because of the bad checksum
        let mut with_bad_checksum = decryption_share;
        with_bad_checksum.validator_checksum.checksum = G1Affine::zero();
        assert!(!with_bad_checksum.verify(
            &pvss_aggregated.aggregate.shares[0],
            &validator_keypairs[0].public_key().encryption_key,
            &dkg.pvss_params.h,
            &ciphertext,
        ));
    }

    #[test_case(4, 4; "number of shares (validators) is a power of 2")]
    #[test_case(7, 7; "number of shares (validators) is not a power of 2")]
    #[test_case(4, 6; "number of validators greater than the number of shares")]
    fn test_dkg_simple_tdec_share_recovery(
        shares_num: u32,
        validators_num: u32,
    ) {
        let rng = &mut test_rng();
        let security_threshold = shares_num / 2 + 1;

        let (dkg, validator_keypairs, messages) =
            setup_dealt_dkg_with_n_validators(
                security_threshold,
                shares_num,
                validators_num,
            );
        let transcripts = messages
            .iter()
            .take(shares_num as usize)
            .map(|m| m.1.clone())
            .collect::<Vec<_>>();
        let public_key = AggregatedTranscript::from_transcripts(&transcripts)
            .unwrap()
            .public_key;
        let ciphertext = ferveo_tdec::encrypt::<E>(
            SecretBox::new(MSG.to_vec()),
            AAD,
            &public_key,
            rng,
        )
        .unwrap();

        // Create an initial shared secret
        let (_, _, old_shared_secret) = create_shared_secret_simple_tdec(
            &dkg,
            AAD,
            &ciphertext.header().unwrap(),
            validator_keypairs.as_slice(),
            &transcripts,
        );

        // TODO: Rewrite this test so that the offboarding of validator
        // is done by recreating a DKG instance with a new set of
        // validators from the Coordinator, rather than modifying the
        // existing DKG instance.

        // Remove one participant from the contexts and all nested structure
        let removed_validator_index = rng.gen_range(0..validators_num);
        let removed_validator_addr = dkg
            .validators
            .iter()
            .find(|(_, v)| v.share_index == removed_validator_index)
            .unwrap()
            .1
            .address
            .clone();
        let mut remaining_validators = dkg.validators.clone();
        remaining_validators.remove(&removed_validator_addr);

        // Remember to remove one domain point too
        let mut domain_points = dkg.domain_point_map();
        domain_points.remove(&removed_validator_index);

        // Now, we're going to recover a new share at a random point,
        // and check that the shared secret is still the same.

        // Our random point:
        let x_r = Fr::rand(rng);

        // Each participant prepares an update for every other participant
        let share_updates = remaining_validators
            .keys()
            .map(|v_addr| {
                let deltas_i = ShareRecoveryUpdate::create_share_updates(
                    &domain_points,
                    &dkg.pvss_params.h.into_affine(),
                    &x_r,
                    dkg.dkg_params.security_threshold(),
                    rng,
                );
                (v_addr.clone(), deltas_i)
            })
            .collect::<HashMap<_, _>>();

        // Participants share updates and update their shares

        // Now, every participant separately:
        let updated_shares: HashMap<u32, _> = remaining_validators
            .values()
            .map(|validator| {
                // Current participant receives updates from other participants
                let updates_for_validator: Vec<_> = share_updates
                    .values()
                    .map(|updates| updates.get(&validator.share_index).unwrap())
                    .cloned()
                    .collect();

                // Each validator uses their decryption key to update their share
                let validator_keypair = validator_keypairs
                    .get(validator.share_index as usize)
                    .unwrap();

                // Creates updated private key shares
                let updated_key_share =
                    AggregatedTranscript::from_transcripts(&transcripts)
                        .unwrap()
                        .aggregate
                        .create_updated_private_key_share(
                            validator_keypair,
                            validator.share_index,
                            updates_for_validator.as_slice(),
                        )
                        .unwrap();
                (validator.share_index, updated_key_share)
            })
            .collect();

        // Now, we have to combine new share fragments into a new share
        let recovered_key_share =
            PrivateKeyShare::recover_share_from_updated_private_shares(
                &x_r,
                &domain_points,
                &updated_shares,
            )
            .unwrap();

        // Get decryption shares from remaining participants
        let mut decryption_shares = remaining_validators
            .values()
            .map(|validator| {
                let validator_keypair = validator_keypairs
                    .get(validator.share_index as usize)
                    .unwrap();
                let decryption_share =
                    AggregatedTranscript::from_transcripts(&transcripts)
                        .unwrap()
                        .aggregate
                        .create_decryption_share_simple(
                            &ciphertext.header().unwrap(),
                            AAD,
                            validator_keypair,
                            validator.share_index,
                        )
                        .unwrap();
                (validator.share_index, decryption_share)
            })
            // We take only the first `security_threshold - 1` decryption shares
            .take((dkg.dkg_params.security_threshold() - 1) as usize)
            .collect::<HashMap<u32, _>>();

        // Create a decryption share from a recovered private key share
        let new_validator_decryption_key = Fr::rand(rng);
        let new_decryption_share = DecryptionShareSimple::create(
            &new_validator_decryption_key,
            &recovered_key_share.0,
            &ciphertext.header().unwrap(),
            AAD,
            &dkg.pvss_params.g_inv(),
        )
        .unwrap();
        decryption_shares.insert(removed_validator_index, new_decryption_share);
        domain_points.insert(removed_validator_index, x_r);

        // We need to make sure that the domain points and decryption shares are ordered
        // by the share index, so that the lagrange basis is calculated correctly

        let mut domain_points_ = vec![];
        let mut decryption_shares_ = vec![];
        for share_index in decryption_shares.keys().sorted() {
            domain_points_.push(
                *domain_points
                    .get(share_index)
                    .ok_or(Error::InvalidShareIndex(*share_index))
                    .unwrap(),
            );
            decryption_shares_.push(
                decryption_shares
                    .get(share_index)
                    .ok_or(Error::InvalidShareIndex(*share_index))
                    .unwrap()
                    .clone(),
            );
        }
        assert_eq!(domain_points_.len(), security_threshold as usize);
        assert_eq!(decryption_shares_.len(), security_threshold as usize);

        let lagrange =
            ferveo_tdec::prepare_combine_simple::<E>(&domain_points_);
        let new_shared_secret = ferveo_tdec::share_combine_simple::<E>(
            &decryption_shares_,
            &lagrange,
        );
        assert_eq!(
            old_shared_secret, new_shared_secret,
            "Shared secret reconstruction failed"
        );
    }

    #[test_case(4, 4; "number of shares (validators) is a power of 2")]
    #[test_case(7, 7; "number of shares (validators) is not a power of 2")]
    #[test_case(4, 6; "number of validators greater than the number of shares")]
    fn test_dkg_simple_tdec_share_refreshing(
        shares_num: u32,
        validators_num: u32,
    ) {
        let rng = &mut test_rng();
        let security_threshold = shares_num / 2 + 1;

        let (dkg, validator_keypairs, messages) =
            setup_dealt_dkg_with_n_validators(
                security_threshold,
                shares_num,
                validators_num,
            );
        let transcripts = messages
            .iter()
            .take(shares_num as usize)
            .map(|m| m.1.clone())
            .collect::<Vec<_>>();
        let public_key = AggregatedTranscript::from_transcripts(&transcripts)
            .unwrap()
            .public_key;
        let ciphertext = ferveo_tdec::encrypt::<E>(
            SecretBox::new(MSG.to_vec()),
            AAD,
            &public_key,
            rng,
        )
        .unwrap();

        // Create an initial shared secret
        let (_, _, old_shared_secret) = create_shared_secret_simple_tdec(
            &dkg,
            AAD,
            &ciphertext.header().unwrap(),
            validator_keypairs.as_slice(),
            &transcripts,
        );

        // Each participant prepares an update for each other participant
        let share_updates = dkg
            .validators
            .keys()
            .map(|v_addr| {
                let deltas_i = ShareRefreshUpdate::create_share_updates(
                    &dkg.domain_point_map(),
                    &dkg.pvss_params.h.into_affine(),
                    dkg.dkg_params.security_threshold(),
                    rng,
                );
                (v_addr.clone(), deltas_i)
            })
            .collect::<HashMap<_, _>>();

        // Participants share updates and update their shares

        // Now, every participant separately:
        let updated_private_key_shares: Vec<_> = dkg
            .validators
            .values()
            .map(|validator| {
                // Current participant receives updates from other participants
                let updates_for_participant: Vec<_> = share_updates
                    .values()
                    .map(|updates| {
                        updates.get(&validator.share_index).cloned().unwrap()
                    })
                    .collect();

                // Each validator uses their decryption key to update their share
                let validator_keypair = validator_keypairs
                    .get(validator.share_index as usize)
                    .unwrap();

                // Creates updated private key shares
                AggregatedTranscript::from_transcripts(&transcripts)
                    .unwrap()
                    .aggregate
                    .create_updated_private_key_share(
                        validator_keypair,
                        validator.share_index,
                        updates_for_participant.as_slice(),
                    )
                    .unwrap()
            })
            .collect();

        // Get decryption shares, now with refreshed private shares:
        let decryption_shares: Vec<DecryptionShareSimple<E>> =
            validator_keypairs
                .iter()
                .enumerate()
                .map(|(share_index, validator_keypair)| {
                    // In order to proceed with the decryption, we need to convert the updated private key shares
                    let private_key_share = &updated_private_key_shares
                        .get(share_index)
                        .unwrap()
                        .inner()
                        .0;
                    DecryptionShareSimple::create(
                        &validator_keypair.decryption_key,
                        private_key_share,
                        &ciphertext.header().unwrap(),
                        AAD,
                        &dkg.pvss_params.g_inv(),
                    )
                    .unwrap()
                })
                // We take only the first `security_threshold` decryption shares
                .take(dkg.dkg_params.security_threshold() as usize)
                .collect();

        // Order of decryption shares is not important, but since we are using low-level
        // API here to performa a refresh for testing purpose, we will not shuffle
        // the shares this time
        // decryption_shares.shuffle(rng);

        let lagrange = ferveo_tdec::prepare_combine_simple::<E>(
            &dkg.domain_points()[..security_threshold as usize],
        );
        let new_shared_secret = ferveo_tdec::share_combine_simple::<E>(
            &decryption_shares[..security_threshold as usize],
            &lagrange,
        );
        assert_eq!(old_shared_secret, new_shared_secret);
    }
}
