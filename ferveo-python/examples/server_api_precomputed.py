from ferveo_py import (
    encrypt,
    combine_decryption_shares_precomputed,
    decrypt_with_shared_secret,
    Keypair,
    PublicKey,
    ExternalValidator,
    Transcript,
    Dkg,
    Ciphertext,
    UnblindingKey,
    DecryptionSharePrecomputed,
    AggregatedTranscript,
    DkgPublicKey,
    DkgPublicParameters,
    SharedSecret,
)


def gen_eth_addr(i: int) -> str:
    return f"0x{i:040x}"


tau = 1
shares_num = 4
# In precomputed variant, security threshold must be equal to shares_num
security_threshold = shares_num
validator_keypairs = [Keypair.random() for _ in range(0, shares_num)]
validators = [
    ExternalValidator(gen_eth_addr(i), keypair.public_key())
    for i, keypair in enumerate(validator_keypairs)
]

# Validators must be sorted by their public key
validators.sort(key=lambda v: v.address)

# Each validator holds their own DKG instance and generates a transcript every
# validator, including themselves
messages = []
for sender in validators:
    dkg = Dkg(
        tau=tau,
        shares_num=shares_num,
        security_threshold=security_threshold,
        validators=validators,
        me=sender,
    )
    messages.append((sender, dkg.generate_transcript()))


# Let's say that we've only received `security_threshold` transcripts
messages = messages[:security_threshold]
transcripts = [transcript for _, transcript in messages]

# Every validator can aggregate the transcripts
dkg = Dkg(
    tau=tau,
    shares_num=shares_num,
    security_threshold=security_threshold,
    validators=validators,
    me=validators[0],
)

server_aggregate = dkg.aggregate_transcripts(transcripts)
assert server_aggregate.verify(shares_num, transcripts)

# Clients can also create aggregates and verify them
client_aggregate = AggregatedTranscript.from_transcripts(transcripts)
assert client_aggregate.verify(shares_num, transcripts)

# We can persist transcripts and the aggregated transcript
transcripts_ser = [bytes(transcript) for _, transcript in messages]
_transcripts_deser = [Transcript.from_bytes(t) for t in transcripts_ser]
agg_transcript_ser = bytes(server_aggregate)
_agg_transcript_deser = AggregatedTranscript.from_bytes(agg_transcript_ser)

# In the meantime, the client creates a ciphertext and decryption request
msg = "abc".encode()
aad = "my-aad".encode()
ciphertext = encrypt(msg, aad, dkg.final_key)

# The client can serialize/deserialize ciphertext for transport
ciphertext_ser = bytes(ciphertext)

# Having aggregated the transcripts, the validators can now create decryption shares
decryption_shares = []
for validator, validator_keypair in zip(validators, validator_keypairs):
    dkg = Dkg(
        tau=tau,
        shares_num=shares_num,
        security_threshold=security_threshold,
        validators=validators,
        me=validator,
    )
    # Assume the aggregated transcript is obtained through deserialization from a side-channel
    agg_transcript_deser = AggregatedTranscript.from_bytes(agg_transcript_ser)
    agg_transcript_deser.verify(shares_num, transcripts)

    # The ciphertext is obtained from the client
    ciphertext_deser = Ciphertext.from_bytes(ciphertext_ser)

    # Create a decryption share for the ciphertext
    decryption_share = agg_transcript_deser.create_decryption_share_precomputed(
        dkg, ciphertext, aad, validator_keypair
    )
    decryption_shares.append(decryption_share)

# Now, the decryption share can be used to decrypt the ciphertext
# This part is in the client API

# The client should have access to the public parameters of the DKG
dkg_public_params_ser = bytes(dkg.public_params)
dkg_public_params_deser = DkgPublicParameters.from_bytes(dkg_public_params_ser)

shared_secret = combine_decryption_shares_precomputed(decryption_shares)

plaintext = decrypt_with_shared_secret(ciphertext, aad, shared_secret, dkg_public_params_deser)
assert bytes(plaintext) == msg

print("Success!")
