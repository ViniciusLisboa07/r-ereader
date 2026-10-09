//! Folha de atalhos (F1). Os textos aqui acompanham as associações em `main::bind_keys`.

pub struct Group {
    pub title: &'static str,
    pub entries: &'static [(&'static str, &'static str)],
}

pub const LIBRARY: &[Group] = &[Group {
    title: "Biblioteca",
    entries: &[
        ("Tab", "Passa entre navegador, busca e livros"),
        ("Setas", "Escolhe um livro (ou um filtro no navegador)"),
        ("Enter", "Abre o livro no leitor (no navegador: aplica o filtro)"),
        ("Home  End", "Primeiro / último"),
        ("PgUp  PgDn", "Uma página acima / abaixo"),
        ("F2", "Edita os metadados"),
        ("Del", "Exclui (pressione de novo para confirmar)"),
        ("Ctrl+F", "Busca na biblioteca; Enter leva aos resultados"),
        ("Ctrl+1  Ctrl+2", "Grade / lista"),
        ("Ctrl+O", "Importa livros"),
        ("Esc", "Cancela a edição ou tira a seleção"),
    ],
}];

pub const READER: &[Group] = &[
    Group {
        title: "Leitura",
        entries: &[
            ("←  →", "Capítulo anterior / próximo"),
            ("↑  ↓", "Rola um pouco"),
            ("PgUp  PgDn  Espaço", "Rola uma tela"),
            ("Home  End", "Início / fim do capítulo"),
            ("Ctrl+=  Ctrl+-", "Aumenta / diminui o texto"),
            ("Ctrl+0", "Tamanho padrão do texto"),
            ("Ctrl+B", "Mostra / esconde a lateral"),
            ("Ctrl+C", "Copia a seleção"),
            ("Esc", "Fecha nota, menu, busca e por fim o livro"),
        ],
    },
    Group {
        title: "Lateral",
        entries: &[
            ("Ctrl+1  Ctrl+2  Ctrl+3", "Sumário / anotações / busca"),
            ("Tab", "Alterna entre o texto e a lateral"),
            ("↑  ↓  Enter", "Escolhe e abre um item"),
        ],
    },
    Group {
        title: "Busca no livro",
        entries: &[
            ("Ctrl+F", "Busca (uma seleção curta vira a busca)"),
            ("Enter  F3  Ctrl+G", "Próximo resultado"),
            ("Shift+F3  Ctrl+Shift+G", "Resultado anterior"),
        ],
    },
];

pub const GLOBAL: Group = Group {
    title: "Geral",
    entries: &[("F1  Ctrl+/", "Mostra / esconde esta folha"), ("Ctrl+Q", "Sai")],
};
