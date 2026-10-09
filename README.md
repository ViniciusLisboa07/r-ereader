# r-ereader

Leitor de ebooks e biblioteca no estilo Calibre, em Rust puro, com interface em
[GPUI](https://gpui.rs) (o framework do Zed), tema sépia e parsing/renderização próprios.

## Rodando

Dependências de sistema no Ubuntu/Debian:

```sh
sudo apt install libxkbcommon-x11-dev libxkbcommon-dev libfreetype-dev libfontconfig-dev libvulkan1
```

```sh
cargo run -p r-ereader                         # abre a biblioteca
cargo run -p r-ereader -- ~/Livros/livro.epub  # abre um EPUB direto no leitor, sem importar
                                              # (se já estiver na biblioteca, usa progresso e destaques)
```

A biblioteca fica em `~/Livros/r-ereader` (ou em `$R_EREADER_LIBRARY`): um `metadata.db` (SQLite) e os
arquivos organizados em `Autor/Título (id)/`, com `cover.jpg` e `thumb.jpg`.

`F1` (ou `Ctrl+/`) mostra a folha com todos os atalhos da tela atual. Tudo dá para fazer sem mouse,
menos selecionar trechos para destacar.

| Onde | Atalho | Ação |
|---|---|---|
| Biblioteca | `Tab` / `Shift+Tab` | Passa entre navegador, busca e livros |
| Biblioteca | setas, `Home` `End`, `PgUp` `PgDn` | Escolhe um livro (no navegador: um filtro; `←` `→` fecham e abrem seções) |
| Biblioteca | `Enter` | Abre o livro (no navegador: aplica o filtro) |
| Biblioteca | `F2` | Edita os metadados |
| Biblioteca | `Del` duas vezes | Exclui o livro |
| Biblioteca | `Ctrl+F` | Busca; `Enter` leva aos resultados |
| Biblioteca | `Ctrl+1` / `Ctrl+2` | Grade / lista |
| Biblioteca | `Ctrl+O` | Importa livros (também dá para arrastar arquivos ou pastas) |
| Biblioteca | `Esc` | Cancela a edição / tira a seleção |
| Leitor | `←` `→` | Capítulo anterior / próximo |
| Leitor | `↑` `↓`, `PgUp` `PgDn` `Espaço`, `Home` `End` | Rola o texto |
| Leitor | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Aumenta / diminui / restaura o tamanho do texto (fica salvo) |
| Leitor | `Ctrl+B` | Mostra / esconde a lateral |
| Leitor | `Ctrl+1` `Ctrl+2` `Ctrl+3` | Sumário / anotações / busca, com `↑` `↓` `Enter` na lista e `Esc` de volta ao texto |
| Leitor | arrastar / duplo clique | Seleciona trecho / palavra (abre o menu de cores) |
| Leitor | clique num destaque | Troca a cor, edita a nota, copia, remove |
| Leitor | `Ctrl+C` | Copia a seleção |
| Leitor | `Ctrl+F` | Busca no livro (uma seleção curta vira a busca) |
| Leitor | `Enter` / `F3` / `Ctrl+G` | Próximo resultado (`Shift+F3` / `Ctrl+Shift+G`: anterior) |
| Leitor | `Esc` | Fecha nota → menu/seleção → limpa a busca → volta à biblioteca (o progresso é salvo) |
| Formulários | `Enter` / `Esc` / `Tab` | Salva / cancela / próximo campo |
| Qualquer | `Ctrl+Q` | Sai |

## Visual

"Fichário": papel sépia, painéis separados por pautas de 1px (no espírito do Zed), cantos quase
retos e números em mono. A grade de livros é uma gaveta de fichas, com o número de catálogo de cada
livro; o painel de detalhes é a ficha pautada. O azul de caneta (`#2b5288`) marca o que está ativo e
o cursor do teclado. As fontes vão embutidas no binário (`crates/app/assets/fonts`, licença OFL):
IBM Plex Sans na interface, IBM Plex Mono nos números e Literata no texto dos livros.

## Obsidian

Os destaques viram uma nota por livro no vault do Obsidian, atualizada automaticamente a cada
destaque criado, editado ou removido (e o progresso, ao fechar o leitor).

- Na primeira execução a pasta é `<vault aberto no Obsidian>/Leituras`. Dá para trocar, exportar
  tudo ou desativar na seção **Obsidian** da barra lateral da biblioteca.
- A nota tem propriedades (`titulo`, `autores` como `[[wikilinks]]`, `serie`, `isbn`, `progresso`…),
  os destaques agrupados por capítulo em callouts `> [!quote|cor]` e âncoras `^rr-<id>` para citar
  um destaque em outra nota: `[[Livro — Autor#^rr-12]]`.
- Só o trecho entre `%% r-ereader:inicio %%` e `%% r-ereader:fim %%` e as propriedades do r-ereader são
  reescritos. Texto e propriedades seus ficam; as tags só são escritas na criação da nota.
- A nota é reencontrada pela propriedade `r_ereader_id`, então pode ser renomeada ou movida
  dentro da pasta. Excluir o livro da biblioteca não apaga a nota.
- As cores dos callouts vêm do snippet `.obsidian/snippets/r-ereader-destaques.css`, criado uma vez;
  ative em *Configurações → Aparência → Trechos de CSS*.

```sh
cargo run -p r-ereader-library --example obsidian -- ~/Livros/r-ereader "<vault>/Leituras"
```

## Estrutura

| Crate | Papel |
|---|---|
| `crates/epub` (`r-ereader-epub`) | Container, OPF (metadados, série, ISBN, manifest, spine), sumário (nav do EPUB 3 e NCX do EPUB 2), DOM XML própria |
| `crates/library` (`r-ereader-library`) | Biblioteca gerenciada: importação com deduplicação por hash, metadados de EPUB e PDF, capas, busca FTS5 sem acentos, coleções, edição (move a pasta), progresso, destaques com notas |
| `crates/app` (`r-ereader`) | Interface GPUI: biblioteca (navegador lateral, grade/lista, detalhes, edição) e leitor |

## Testes

```sh
cargo test -p r-ereader-epub -p r-ereader-library
cargo test -p r-ereader      # testes de interface headless (TestAppContext do GPUI)
```

Ferramentas de diagnóstico do crate `epub`:

```sh
cargo run -p r-ereader-epub --example inspect -- livro.epub     # metadados + sumário
cargo run -p r-ereader-epub --example check_all -- *.epub       # parse de todos os capítulos
```

## Roteiro

1. ~~`epub`: container, OPF, spine, sumário~~
2. ~~`library` + hub: importação, busca, filtros, edição, coleções, progresso~~
   ~~destaques em 4 cores com notas e aba de anotações; exportação para o Obsidian~~
   ~~busca no livro inteiro, sem acentos/maiúsculas, com frase exata e palavras separadas~~
3. `doc` + `layout`: árvore de documento semântica, quebra de linha e paginação próprias,
   com o shaping de texto do GPUI por trás de uma trait
4. `style`: subconjunto de CSS (seletores, cascata, herança) + CSS do usuário
5. Imagens no fluxo, hifenização, temas, destaques; capa de PDF (renderizar a 1ª página); leitor de PDF;
   importar biblioteca do Calibre

A área de leitura atual (`crates/app/src/preview.rs`) é provisória: extrai blocos de texto
sem CSS só para enxergar o conteúdo até o motor de layout existir.

## Busca no livro

Na primeira busca o livro inteiro é indexado em segundo plano (`crates/app/src/search.rs`).
A comparação ignora maiúsculas, acentos, aspas/apóstrofos/travessões tipográficos e espaços
repetidos: `nao` acha "Não", `d'avila` acha "d’Ávila". Com várias palavras, os parágrafos com a
frase exata vêm primeiro; depois, os que têm todas as palavras fora de ordem. O primeiro `Enter`
vai ao resultado mais próximo a partir do capítulo atual.

## Notas

- Destaques guardam o capítulo, o intervalo no texto do capítulo e o trecho citado. Se a extração
  de texto mudar (por exemplo, quando o motor de layout próprio substituir o atual), o trecho é
  reencontrado pela citação e a posição nova é gravada (`r_ereader_library::anchor`).
- `patches/xattr`: o `xattr` 0.2.3, puxado pelo GPUI, não compila com `libc` >= 0.2.190
  (o `ENOATTR` foi removido no Linux). O patch local troca pelo `ENODATA` equivalente.
