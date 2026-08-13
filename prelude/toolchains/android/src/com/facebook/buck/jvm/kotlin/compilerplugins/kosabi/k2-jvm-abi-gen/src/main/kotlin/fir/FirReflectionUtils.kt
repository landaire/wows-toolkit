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
@file:Suppress("OPT_IN_USAGE_ERROR")
@file:OptIn(
    com.facebook.DirectDeclarationsAccessCompat::class,
)

package com.facebook

import java.lang.reflect.Field
import org.jetbrains.kotlin.fir.declarations.FirDeclaration
import org.jetbrains.kotlin.fir.declarations.FirRegularClass
import org.jetbrains.kotlin.fir.expressions.FirAnnotation
import org.jetbrains.kotlin.fir.expressions.FirAnnotationCall

/**
 * Encapsulates all reflection access to Kotlin FIR internals.
 *
 * FIR's API treats many fields as read-only, but for ABI generation we need to mutate annotations,
 * supertype refs, declarations, and property initializers. Centralizing reflection access here
 * makes it easier to maintain when FIR internals change between Kotlin compiler versions.
 */
@SuppressWarnings("PackageLocationMismatch")
internal object FirReflectionUtils {

  /**
   * Find a field by name, searching up the class hierarchy. Needed because the field may be in a
   * parent class (e.g., FirDefaultPropertyBackingField extends FirBackingFieldImpl which has the
   * annotations field).
   */
  fun findFieldInHierarchy(clazz: Class<*>, fieldName: String): Field? {
    var current: Class<*>? = clazz
    while (current != null) {
      val field = current.declaredFields.find { it.name == fieldName }
      if (field != null) return field
      current = current.superclass
    }
    return null
  }

  /**
   * Get the mutable annotations list from a FIR declaration via reflection. Returns null if
   * reflection fails.
   *
   * FIR annotations are stored in a MutableOrEmptyList value class wrapping MutableList<T>?. We
   * need to unwrap this to get the actual mutable list.
   */
  fun getMutableAnnotations(declaration: FirDeclaration): MutableList<FirAnnotation>? {
    return try {
      val annotationsField = declaration.javaClass.getDeclaredField("annotations")
      annotationsField.isAccessible = true
      val annotationsWrapper = annotationsField.get(declaration) ?: return null

      // MutableOrEmptyList is a value class wrapping MutableList<T>?
      val listField = annotationsWrapper.javaClass.getDeclaredField("list")
      listField.isAccessible = true
      @Suppress("UNCHECKED_CAST")
      listField.get(annotationsWrapper) as? MutableList<FirAnnotation>
    } catch (_: Exception) {
      null
    }
  }

  /**
   * Get the mutable annotations list from a FIR declaration via field hierarchy search. This
   * variant searches the class hierarchy for the "annotations" field, which is needed for types
   * like FirDefaultPropertyBackingField where the field is in a parent class. Returns the list cast
   * as MutableList<FirAnnotationCall>.
   */
  fun getMutableAnnotationCalls(declaration: FirDeclaration): MutableList<FirAnnotationCall>? {
    return try {
      val annotationsField =
          findFieldInHierarchy(declaration.javaClass, "annotations") ?: return null
      annotationsField.isAccessible = true
      @Suppress("UNCHECKED_CAST")
      annotationsField.get(declaration) as? MutableList<FirAnnotationCall>
    } catch (_: Exception) {
      null
    }
  }

  /**
   * Get the mutable superTypeRefs list from a FIR class via reflection. Handles both direct
   * MutableList and MutableOrEmptyList wrapper cases.
   */
  fun getMutableSuperTypeRefs(
      firClass: FirRegularClass
  ): MutableList<org.jetbrains.kotlin.fir.types.FirTypeRef>? {
    return try {
      val superTypeRefsField = firClass.javaClass.getDeclaredField("superTypeRefs")
      superTypeRefsField.isAccessible = true
      val superTypeRefsValue = superTypeRefsField.get(firClass) ?: return null

      @Suppress("UNCHECKED_CAST")
      when (superTypeRefsValue) {
        is MutableList<*> ->
            superTypeRefsValue as MutableList<org.jetbrains.kotlin.fir.types.FirTypeRef>
        else -> {
          // Fallback: try to unwrap from MutableOrEmptyList if needed
          val listField =
              superTypeRefsValue.javaClass.declaredFields.find { it.name == "list" } ?: return null
          listField.isAccessible = true
          listField.get(superTypeRefsValue)
              as? MutableList<org.jetbrains.kotlin.fir.types.FirTypeRef>
        }
      }
    } catch (_: Exception) {
      null
    }
  }

  /** Get the mutable declarations list from a FIR class via reflection. */
  fun getMutableDeclarations(firClass: FirRegularClass): MutableList<FirDeclaration>? {
    return try {
      val declarationsField = firClass.javaClass.getDeclaredField("declarations")
      declarationsField.isAccessible = true
      @Suppress("UNCHECKED_CAST")
      declarationsField.get(firClass) as? MutableList<FirDeclaration>
    } catch (_: Exception) {
      null
    }
  }

  /** Clear a property's initializer via reflection. */
  fun clearPropertyInitializer(property: org.jetbrains.kotlin.fir.declarations.FirProperty) {
    try {
      val initializerField = property.javaClass.getDeclaredField("initializer")
      initializerField.isAccessible = true
      initializerField.set(property, null)
    } catch (_: Exception) {
      // If reflection fails, skip this property
    }
  }
}
