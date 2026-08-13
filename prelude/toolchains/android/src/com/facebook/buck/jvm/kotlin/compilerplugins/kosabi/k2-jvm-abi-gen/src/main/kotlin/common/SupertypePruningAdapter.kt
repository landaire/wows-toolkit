/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

@file:SuppressWarnings("PackageLocationMismatch")

package com.facebook

/**
 * Abstraction for supertype pruning that works across FIR and IR representations.
 *
 * Both FIR and IR need to:
 * 1. Identify private supertypes
 * 2. Remove them from the supertype list
 * 3. Collect method signatures from stripped interfaces
 * 4. Convert fake override methods to real methods with stub bodies
 *
 * This adapter provides a uniform API so the pruning algorithm can be written once and applied to
 * both representations.
 *
 * @param TClass The class representation (FirRegularClass or IrClass)
 * @param TType The type representation (FirTypeRef or IrType)
 * @param TMethod The method representation (FirSimpleFunction or IrSimpleFunction)
 */
internal interface SupertypePruningAdapter<TClass, TType, TMethod> {
  /** Get all supertypes of the given class. */
  fun getSupertypes(clazz: TClass): List<TType>

  /** Check if the given supertype references a private class. */
  fun isPrivate(type: TType): Boolean

  /** Remove a supertype from the class's supertype list. */
  fun removeSupertype(clazz: TClass, type: TType)

  /** List all public/protected methods from the interface referenced by this type. */
  fun listInterfaceMethods(type: TType): List<TMethod>

  /** Get method signatures already declared in the class. */
  fun getExistingMethodSignatures(clazz: TClass): Set<MethodSignature>

  /** Get the method signature for matching. */
  fun getMethodSignature(method: TMethod): MethodSignature

  /** Add a copy of the method to the target class. */
  fun addMethodCopy(clazz: TClass, method: TMethod)
}

/**
 * Common supertype pruning algorithm.
 *
 * Given an adapter for either FIR or IR, performs the standard pruning:
 * 1. Find private supertypes
 * 2. Collect methods from those interfaces
 * 3. Remove the private supertypes
 * 4. Add method copies for methods not already declared
 */
internal fun <TClass, TType, TMethod> prunePrivateSupertypes(
    clazz: TClass,
    adapter: SupertypePruningAdapter<TClass, TType, TMethod>,
) {
  val supertypes = adapter.getSupertypes(clazz)
  val privateSupertypes = supertypes.filter { adapter.isPrivate(it) }

  if (privateSupertypes.isEmpty()) return

  // Collect methods from stripped interfaces before removing supertypes
  val interfaceMethods = privateSupertypes.flatMap { adapter.listInterfaceMethods(it) }

  // Remove private supertypes
  for (type in privateSupertypes) {
    adapter.removeSupertype(clazz, type)
  }

  // Add method copies for methods not already declared
  if (interfaceMethods.isNotEmpty()) {
    val existingSignatures = adapter.getExistingMethodSignatures(clazz)
    for (method in interfaceMethods) {
      if (adapter.getMethodSignature(method) !in existingSignatures) {
        adapter.addMethodCopy(clazz, method)
      }
    }
  }
}
