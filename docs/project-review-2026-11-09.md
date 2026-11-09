# Zila Lang: pendências da revisão de 2026-11-09

Este arquivo guarda **só o que ainda está aberto** da revisão feita no commit `c7401ab`. O que foi resolvido saiu daqui e está registrado na mudança OpenSpec [`2026-11-09-repo-hygiene-and-ci`](../openspec/changes/archive/2026-11-09-repo-hygiene-and-ci) e no `CHANGELOG.md` (seção Unreleased).

Resolvido (resumo, detalhes na mudança): roadmap do README reescrito e protegido por teste, contagem de testes, `rename-language` removido, tag local `v0.1.0`, workflow de CI, clippy zerado com `-D warnings`, `docs/Zila.docx` removido, como recriar `.claude/.agent/.opencode` documentado, spec `threads` marcada como não atendida no Linux.

## Verificações que faltam

- **Primeira execução do CI no GitHub**, principalmente o job Linux: foi escrito sem runner disponível, e alguns alvos de teste podem assumir Windows e precisar de guarda de skip.
- **Publicar a tag**: `git push origin v0.1.0` (a tag é local, em `f8529fa`). Próxima versão sugerida: `0.2.0` (renomeação do comando é quebra).
- **`tests/async.rs`** não foi executado nesta rodada (regra de pular nas execuções de rotina); o job `async` do CI vai cobrir.

## Pendências por área

### Compilador
1. **Pipeline tipado inacabado.** `type_of` só é conferido contra a emissão em builds de teste. Falta: emissão consumir árvore tipada, registrar o alvo de cada chamada, remover a tipagem duplicada e os `unreachable!()` que ela torna impossíveis, e só depois o cache de pacotes (incremental). Manter `tools/asm-snapshot.py` como portão.
2. **Auditoria de `unwrap()`** (120 em `src/`): separar os de teste dos de caminho de produção.
3. **Política `SAFETY:`** para os 49 usos de `unsafe` em `src/`.

### Plataforma
4. **Camada de threads no Linux** (`clone`, `futex`, scheduler por thread sobre `epoll`), pesos fracos e identidade em `shared T`. A spec `threads` já diz que o requisito não é atendido.
5. **Paridade Windows/Linux de `std`**: `tests/data/std_parity_known.txt` tem 63 linhas, sobretudo em `task`; a lista só pode encolher.
6. `target/` com 7,7 GB: considerar `debug = "line-tables-only"` no perfil dev ou `cargo clean` periódico.
7. Novos alvos (macOS, ARM64, WebAssembly) só depois do pipeline tipado e do CI estável; ARM64 é a maior alavanca para "IoT e mobile".

### Ferramentas
8. **LSP**: hover, definição, completion e erros de tipo (o `type_of` é a base natural).
9. **VS Code**: gerar o `.vsix` e fazer o smoke test manual.
10. **TLS**: servidor, `close_notify`, leitor com buffer.

### Segurança e confiança
11. **Modelo de ameaça e `SECURITY.md`**: comptime executa código no build, pacotes por git (hash fixa a fonte, mas `effects.rs` só mostra `unsafe`/`extern`/`asm`), canal de reporte.
12. **Hardening**: vetores oficiais (NIST/RFC) para `crypto` e TLS (não foi verificado se `tests/crypto.rs` os usa), fuzzing do lexer/parser/comptime, propriedade de idempotência no formatador, testes do escritor de PE/ELF.
13. Servidor `web` sem TLS, HTTP/2, cookies, multipart: aceitável para desenvolvimento, documentado.

### Processo e escopo
14. **Congelar bibliotecas novas** até endurecer as fundações: semântica de memória e `shared` por escrito, política 1.0 de quebras, lista de saída de pré-alpha. Em 4 dias entraram async, threads, comptime, asm, OS, TLS, UI, web, IA, MCP, RAG, dois bancos, SQL blocks, LSP e extensão.
15. **Bus factor**: autor único aparente; `GOVERNANCE.md` descreve papéis que ainda não existem na prática. Abrir issues "good first issue" a partir da tabela de lacunas do README.
16. **Docker vs servidores locais**: `tests/docker/` convive com testes em servidores locais; documentar qual caminho é o suportado e como o CI (sem bancos) se comporta.

### Clareza
17. README ainda longo (~470 linhas) e com células enormes na tabela de estado (JSON, Web, pacotes, Linux): mover detalhe para as páginas do site e considerar um `ROADMAP.md`.
18. Regra única para "Zila" vs "Zila Lang" (39 ocorrências de "Zila Lang" no README).
19. **Documento de arquitetura do compilador** (lexer, parser, checagens, comptime, prune, codegen, PE/ELF) em `docs/`, hoje espalhado nos `design.md` arquivados; e um `docs/README.md` indexando `zila.ebnf`, `zila-ffi.md` e este arquivo.
20. Entre os avisos de clippy permitidos por `#[allow]` no item (índices paralelos, `too_many_arguments` em três coletores de `noalloc.rs`, `to_text`), reavaliar se um refator pequeno os elimina.

## Estado verificado após as correções

`cargo clippy --all-targets -- -D warnings` limpo; todos os alvos de teste exceto `async`: 1.017 passaram, 0 falharam, 1 ignorado (o novo teste do roadmap incluído).
