/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

@file:Suppress("PackageLocationMismatch")

package com.facebook

// DeprecatedForRemovalCompilerApi does not exist in Kotlin 2.1. This is a no-op
// annotation so @OptIn(DeprecatedForRemovalCompilerApiCompat::class) compiles.
@RequiresOptIn annotation class DeprecatedForRemovalCompilerApiCompat

// DirectDeclarationsAccess does not exist in Kotlin 2.1. Declarations are freely accessible.
// This is a no-op annotation so @OptIn(DirectDeclarationsAccessCompat::class) compiles.
@RequiresOptIn annotation class DirectDeclarationsAccessCompat
