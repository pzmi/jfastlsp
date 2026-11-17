# multi-module

A synthetic three-module Java project for the jdtls comparison arm of `jfl-bench` (BENCH-03):
`ledger-core` holds the model, `ledger-api` a service over it, `ledger-app` a client of both, so
definitions and references cross module boundaries. It has no build file on purpose: jdtls is
run with its Gradle and Maven imports off (a jdtls import runs the project's build), and the
arm passes the three source roots as `java.project.sourcePaths`. Written for this repository;
no corpus code.
