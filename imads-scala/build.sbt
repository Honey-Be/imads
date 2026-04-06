val scala3Version = "3.6.4"

lazy val shared = crossProject(JVMPlatform, JSPlatform, NativePlatform)
  .crossType(CrossType.Pure)
  .in(file("shared"))
  .settings(
    name := "imads-shared",
    scalaVersion := scala3Version,
  )

lazy val jvm = project.in(file("jvm"))
  .settings(
    name := "imads-jvm",
    scalaVersion := scala3Version,
  )
  .dependsOn(shared.jvm)

lazy val js = project.in(file("js"))
  .enablePlugins(ScalaJSPlugin)
  .settings(
    name := "imads-js",
    scalaVersion := scala3Version,
    scalaJSUseMainModuleInitializer := false,
    scalaJSLinkerConfig ~= { _.withModuleKind(ModuleKind.ESModule) },
  )
  .dependsOn(shared.js)

lazy val native = project.in(file("native"))
  .enablePlugins(ScalaNativePlugin)
  .settings(
    name := "imads-native",
    scalaVersion := scala3Version,
    nativeLinkingOptions += s"-L${baseDirectory.value}/../../target/release",
  )
  .dependsOn(shared.native)

lazy val root = project.in(file("."))
  .aggregate(jvm, js, native)
  .settings(
    name := "imads-scala",
    publish / skip := true,
  )
