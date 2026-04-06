plugins {
    kotlin("multiplatform") version "2.1.20"
}

group = "io.imads"
version = "0.1.0"

kotlin {
    jvm()
    js(IR) {
        browser()
        nodejs()
    }
    linuxX64 {
        compilations.getByName("main") {
            cinterops {
                val imads_ffi by creating {
                    defFile("src/nativeMain/cinterop/imads_ffi.def")
                    includeDirs("../imads-ffi/include")
                }
            }
        }
    }

    sourceSets {
        val commonMain by getting
        val jvmMain by getting {
            dependencies {
                implementation(files("../imads-jni/java/target"))
            }
        }
        val jsMain by getting {
            dependencies {
                implementation(npm("imads-wasm", "../imads-wasm/pkg"))
            }
        }
    }
}
