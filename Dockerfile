FROM ubuntu:22.04 as builder

LABEL maintainer="Nikola Whallon <nikola@deepgram.com>"

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        clang \
        curl \
        libpq-dev \
        libssl-dev \
        pkg-config

COPY rust-toolchain /rust-toolchain
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain $(cat /rust-toolchain) && \
    . $HOME/.cargo/env

COPY . /buttercup

RUN . $HOME/.cargo/env && \
    cargo install --path /buttercup --root /

FROM ubuntu:22.04

LABEL maintainer="Nikola Whallon <nikola@deepgram.com>"

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        libpq5 \
        libssl3 && \
    apt-get clean

COPY --from=builder /bin/buttercup /bin/buttercup

ENTRYPOINT ["/bin/buttercup"]
