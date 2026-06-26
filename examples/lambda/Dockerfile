# Deploy php-quickjs on AWS Lambda as a container image (Bref's recommended path
# for custom binaries). Build for the architecture you deploy to: linux/arm64
# (Graviton) or linux/amd64.
#
#   docker build --platform linux/arm64 \
#     --build-arg EXT=php-quickjs-v0.0.1-php8.4-lambda-bref-arm64.so -t my-app .
#
# Always pin the Bref image MAJOR version. Match the PHP minor (8.4/8.5) and the
# architecture to the .so you copy in.
FROM bref/php-84:3

# The prebuilt extension, downloaded from a php-quickjs release. Pass the file
# name matching your PHP version + arch via --build-arg EXT=...
ARG EXT

# /opt/bref/extensions is Bref's extension_dir, and /opt/bref/etc/php/conf.d is
# scanned for *.ini — so a bare `extension=quickjs.so` is enough.
COPY ${EXT} /opt/bref/extensions/quickjs.so
RUN echo 'extension=quickjs.so' > /opt/bref/etc/php/conf.d/ext-quickjs.ini

# Your application code (and `composer install`ed vendor/).
COPY . /var/task
