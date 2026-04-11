plugins {
    kotlin("jvm") version "2.1.0"
}

group = "io.imads"
version = "2.0.1"

java {
    toolchain {
        languageVersion.set(JavaLanguageVersion.of(22))
    }
}

kotlin {
    jvmToolchain(22)
}

dependencies {
    testImplementation(kotlin("test"))
}
