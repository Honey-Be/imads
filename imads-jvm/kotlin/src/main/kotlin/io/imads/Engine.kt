package io.imads

import java.lang.foreign.*
import java.lang.foreign.MemoryLayout.PathElement
import java.lang.invoke.MethodHandle

/**
 * IMADS engine wrapper using JDK 22+ Foreign Function & Memory API.
 *
 * Loads the native `imads_jvm` shared library and calls C functions directly
 * without JNI overhead.
 */
class Engine private constructor(private val handle: MemorySegment) : AutoCloseable {
    companion object {
        private val linker = Linker.nativeLinker()
        private val arena = Arena.ofAuto()

        private val lib: SymbolLookup by lazy {
            SymbolLookup.libraryLookup(System.mapLibraryName("imads_jvm"), arena)
        }

        private val engineNew: MethodHandle by lazy {
            linker.downcallHandle(
                lib.find("imads_engine_new").orElseThrow(),
                FunctionDescriptor.of(ValueLayout.ADDRESS)
            )
        }

        private val engineFree: MethodHandle by lazy {
            linker.downcallHandle(
                lib.find("imads_engine_free").orElseThrow(),
                FunctionDescriptor.ofVoid(ValueLayout.ADDRESS)
            )
        }

        private val configFromPreset: MethodHandle by lazy {
            linker.downcallHandle(
                lib.find("imads_config_from_preset").orElseThrow(),
                FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS)
            )
        }

        private val configFree: MethodHandle by lazy {
            linker.downcallHandle(
                lib.find("imads_config_free").orElseThrow(),
                FunctionDescriptor.ofVoid(ValueLayout.ADDRESS)
            )
        }

        fun create(): Engine {
            val ptr = engineNew.invoke() as MemorySegment
            return Engine(ptr)
        }

        fun presetNames(): List<String> = listOf(
            "legacy_baseline", "balanced", "conservative", "throughput"
        )
    }

    override fun close() {
        engineFree.invoke(handle)
    }
}

/** Sealed interface for compile-time objective count safety. */
sealed interface ObjectiveCount {
    data object N1 : ObjectiveCount
    data object N2 : ObjectiveCount
    data object N3 : ObjectiveCount
}
