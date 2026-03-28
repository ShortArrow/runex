# Third-Party Licenses

`runex` itself is licensed under the MIT License.

This project also depends on third-party crates that are distributed under their
own licenses. The current dependency tree is primarily MIT or Apache-2.0, with
one MPL-2.0-licensed transitive dependency identified in the current build:

## MPL-2.0 Dependency

### option-ext 0.2.0

- License: Mozilla Public License 2.0
- Repository: <https://github.com/soc/option-ext>
- Usage path: `runex-core -> dirs -> dirs-sys -> option-ext`

This dependency is used transitively through the `dirs` crate for platform
directory resolution.

For the full MPL-2.0 license text, see:

- <https://www.mozilla.org/en-US/MPL/2.0/>

## Notes

- The `runex` source code remains MIT-licensed.
- Third-party dependencies keep their original licenses.
- If you redistribute binaries, include this file together with the project
  `LICENSE`.
