# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is dual-licensed under either the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree or the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree. You may select, at your option, one of the
# above-listed licenses.

# pyre-strict

import argparse
import base64
import json
from pathlib import Path
from typing import Optional

from apple.tools.code_signing.codesign_bundle import (
    selection_profile_context_from_signing_context,
)

from .signing_context import (
    add_args_for_signing_context,
    signing_context_and_selected_identity_from_args,
)


def _build_signing_info_json(
    args: argparse.Namespace,
    selected_identity: Optional[str],
    selection_profile_context: Optional[object],
) -> dict:
    if not args.codesign:
        return {}

    signing_info: dict = {
        "codesign_type": "adhoc" if args.ad_hoc else "distribution",
    }

    if selected_identity:
        signing_info["codesign_identity"] = selected_identity

    if selection_profile_context:
        selected_profile_info = selection_profile_context.selected_profile_info
        profile_metadata = selected_profile_info.profile
        signing_info["provisioning_profile"] = {
            "uuid": profile_metadata.uuid,
            "file_name": profile_metadata.file_path.name,
        }
        signing_info["signing_certificate"] = {
            "fingerprint": selected_profile_info.identity.fingerprint,
            "subject_common_name": selected_profile_info.identity.subject_common_name,
        }

    return signing_info


def _main() -> None:
    parser = argparse.ArgumentParser(
        description="Tool which outputs the signing context for an apple_bundle().",
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="Path to the output JSON file.",
    )
    parser.add_argument(
        "--signing-info-output",
        required=False,
        type=Path,
        help="Path to the output JSON file for simplified signing identity metadata.",
    )
    add_args_for_signing_context(parser)

    args = parser.parse_args()
    signing_context, selected_identity = (
        signing_context_and_selected_identity_from_args(args)
    )

    selection_profile_context = selection_profile_context_from_signing_context(
        signing_context
    )

    with open(args.output, "w") as output_file:
        signing_context_json_obj = {
            "version": 1,
        }

        if selected_identity:
            # Adhoc and Developer ID builds will only have `codesign_identity`
            # (in which case, it would the human readable identity).
            #
            # For provisioned builds (i.e., with a prov profile), it would be the
            # signing cert fingerprint (i.e., SHA1 hash of cert in DER format)
            signing_context_json_obj["codesign_identity"] = selected_identity

        if selection_profile_context:
            selected_profile_info = selection_profile_context.selected_profile_info
            profile_metadata = selected_profile_info.profile
            with open(profile_metadata.file_path, "rb") as prov_profile_file:
                prov_profile_as_base64_utf8 = base64.standard_b64encode(
                    prov_profile_file.read()
                ).decode()

            signing_context_json_obj["provisioning_profile"] = {
                "uuid": profile_metadata.uuid,
                "identity": {
                    "fingerprint": selected_profile_info.identity.fingerprint,
                    "subject_common_name": selected_profile_info.identity.subject_common_name,
                },
                "file_name": profile_metadata.file_path.name,
                "file_data_base64": prov_profile_as_base64_utf8,
            }

        json.dump(
            signing_context_json_obj,
            output_file,
            indent=4,
        )

    if args.signing_info_output:
        signing_info = _build_signing_info_json(
            args, selected_identity, selection_profile_context
        )
        with open(args.signing_info_output, "w") as signing_info_file:
            json.dump(signing_info, signing_info_file, indent=4)


if __name__ == "__main__":
    _main()
