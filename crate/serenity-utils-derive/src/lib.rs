//! Procedural macros for the `serenity-utils` crate.

#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_qualifications, warnings)]

use {
    std::ops::RangeInclusive,
    convert_case::{
        Case,
        Casing as _,
    },
    if_chain::if_chain,
    itertools::Itertools as _,
    proc_macro::TokenStream,
    proc_macro2::Span,
    quote::{
        quote,
        quote_spanned,
    },
    syn::{
        AttributeArgs,
        FnArg,
        Ident,
        ItemConst,
        ItemFn,
        ItemUse,
        Lit,
        LitInt,
        LitStr,
        Meta,
        MetaList,
        MetaNameValue,
        NestedMeta,
        Pat,
        PatIdent,
        PatType,
        Path,
        PathArguments,
        ReturnType,
        Token,
        Type,
        TypePath,
        TypeReference,
        parse::{
            Parse,
            ParseStream,
            Parser as _,
        },
        parse_macro_input,
        parse_quote,
        punctuated::Punctuated,
        spanned::Spanned as _,
    },
};

enum Port {
    Const(ItemConst),
    Fn(ItemFn),
}

impl Parse for Port {
    fn parse(input: ParseStream<'_>) -> syn::Result<Port> {
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![const]) {
            input.parse().map(Port::Const)
        } else if lookahead.peek(Token![fn]) {
            input.parse().map(Port::Fn)
        } else {
            Err(lookahead.error())
        }
    }
}

impl quote::ToTokens for Port {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Port::Const(item) => item.to_tokens(tokens),
            Port::Fn(item) => item.to_tokens(tokens),
        }
    }
}

fn parser(input: ParseStream<'_>) -> syn::Result<(ItemUse, Port, Vec<ItemFn>)> {
    let uses = input.parse()?;
    let port = input.parse()?;
    let mut commands = vec![];
    while !input.is_empty() {
        commands.push(input.parse()?);
    }
    Ok((uses, port, commands))
}

#[proc_macro]
pub fn ipc(input: TokenStream) -> TokenStream {
    let (uses, port, commands) = match parser.parse(input) {
        Ok(commands) => commands,
        Err(e) => return e.to_compile_error().into()
    };
    let addr_fn = {
        let port = match port {
            Port::Const(ref item) => { let ident = &item.ident; quote!(#ident) }
            Port::Fn(ref item) => { let ident = &item.sig.ident; quote!(#ident()) }
        };
        quote! {
            /// The address and port where the bot listens for IPC commands.
            fn addr() -> ::std::net::SocketAddr {
                ::std::net::SocketAddr::from(([127, 0, 0, 1], #port))
            }
        }
    };
    let fn_names = commands.iter()
        .map(|cmd| &cmd.sig.ident)
        .collect::<Vec<_>>();
    let cmd_names = fn_names.iter()
        .map(|fn_name| fn_name.to_string().replace('_', "-"))
        .collect::<Vec<_>>();
    let parse_args = commands.iter()
        .map(|cmd| (1..cmd.sig.inputs.len()).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let arg_types = commands.iter()
        .map(|cmd| cmd.sig.inputs.iter().skip(1).map(|arg| {
            let arg = match arg {
                FnArg::Receiver(_) => panic!("IPC command can't have a `self` argument"), //TODO compile error instead of panic
                FnArg::Typed(arg) => arg,
            };
            &arg.ty
        }).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let client_fns = commands.iter()
        .zip(&cmd_names)
        .map(|(cmd, cmd_name)| {
            let docs = cmd.attrs.iter().filter(|attr| attr.path.is_ident("doc")).collect::<Vec<_>>();
            let fn_name = &cmd.sig.ident;
            let typed_args = cmd.sig.inputs.iter().skip(1).collect::<Vec<_>>();
            let untyped_args = cmd.sig.inputs.iter().skip(1).map(|arg| {
                let arg = match arg {
                    FnArg::Receiver(_) => panic!("IPC command can't have a `self` argument"), //TODO compile error instead of panic
                    FnArg::Typed(arg) => arg,
                };
                &arg.pat
            }).collect::<Vec<_>>();
            quote! {
                #(#docs)*
                pub fn #fn_name(#(#typed_args),*) -> Result<(), Error> {
                    let received = send(vec![#cmd_name.to_owned() #(, #untyped_args.to_string())*])?;
                    if received != #cmd_name {
                        return Err(Error::WrongReply {
                            received,
                            expected: format!(#cmd_name),
                        })
                    }
                    Ok(())
                }
            }
        })
        .collect::<Vec<_>>();
    TokenStream::from(quote! {
        use {
            ::std::io::prelude::*,
            ::serenity_utils::{
                futures::prelude::*,
                tokio::io::AsyncWriteExt as _,
            },
        };
        #uses

        #port

        #[derive(Debug, ::serenity_utils::derive_more::From)]
        pub enum Error {
            /// An IPC command's argument could not be parsed.
            #[from(ignore)]
            ArgParse(String),
            Io(::std::io::Error),
            /// Returned if a Serenity context was required outside of an event handler but the `ready` event has not been received yet.
            MissingContext,
            /// The command reply did not end in a line break.
            MissingNewline,
            /// Returned from `listen` if a command line was not valid shell lexer tokens.
            #[from(ignore)]
            Shlex(String),
            /// Returned from `listen` if an unknown command is received.
            #[from(ignore)]
            UnknownCommand(Vec<String>),
        }

        impl ::std::fmt::Display for Error {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    Error::ArgParse(msg) => write!(f, "error parsing IPC command argument: {}", msg),
                    Error::Io(e) => e.fmt(f),
                    Error::MissingContext => write!(f, "Serenity context not available before ready event"),
                    Error::MissingNewline => write!(f, "the reply to an IPC command did not end in a newline"),
                    Error::Shlex(line) => write!(f, "failed to parse IPC command line: {}", line),
                    Error::UnknownCommand(args) => write!(f, "unknown command: {:?}", args),
                }
            }
        }

        impl ::std::error::Error for Error {}

        #addr_fn

        async fn handle_client(ctx_fut: &::serenity_utils::RwFuture<::serenity::client::Context>, stream: ::serenity_utils::tokio::net::TcpStream) -> Result<(), Error> {
            let mut last_error = Ok(());
            let mut buf = String::default();
            let (reader, mut writer) = stream.into_split();
            let mut lines = ::serenity_utils::tokio_stream::wrappers::LinesStream::new(::serenity_utils::tokio::io::AsyncBufReadExt::lines(::serenity_utils::tokio::io::BufReader::new(reader)));
            while let Some(line) = lines.next().await {
                let line = match line {
                    Ok(line) => line,
                    Err(e) => if e.kind() == ::std::io::ErrorKind::ConnectionReset {
                        break // connection reset by peer, consider the IPC session terminated
                    } else {
                        return Err(Error::Io(e))
                    }
                };
                buf.push_str(&line);
                let args = match ::serenity_utils::shlex::split(&buf) {
                    Some(args) => {
                        last_error = Ok(());
                        buf.clear();
                        args
                    }
                    None => {
                        last_error = Err(Error::Shlex(line));
                        buf.push('\n');
                        continue
                    }
                };
                match &args[0][..] {
                    #(
                        #cmd_names => {
                            let ctx = ctx_fut.read().await;
                            match #fn_names(&*ctx #(, args[#parse_args].parse::<#arg_types>().map_err(|e| Error::ArgParse(e.to_string()))?)*).await {
                                Ok(()) => writer.write_all(&format!("{}\n", #cmd_names).into_bytes()).await?,
                                Err(msg) => writer.write_all(&format!("{}\n", msg).into_bytes()).await?,
                            }
                        }
                    )*
                    _ => return Err(Error::UnknownCommand(args)),
                }
            }
            last_error
        }

        pub async fn listen<Fut: ::std::future::Future<Output = ()>>(ctx_fut: ::serenity_utils::RwFuture<::serenity::client::Context>, notify_thread_crash: &impl Fn(::std::string::String, Box<dyn ::std::error::Error + ::core::marker::Send + 'static>, ::core::option::Option<::core::time::Duration>) -> Fut) -> ::std::io::Result<::std::convert::Infallible> {
            let mut listener = ::serenity_utils::tokio_stream::wrappers::TcpListenerStream::new(::serenity_utils::tokio::net::TcpListener::bind(addr()).await?);
            while let Some(stream) = listener.next().await {
                let stream = match stream.map_err(Error::Io) {
                    Ok(stream) => stream,
                    Err(e) => {
                        notify_thread_crash(format!("IPC client"), Box::new(e), None).await;
                        continue
                    }
                };
                if let Err(e) = handle_client(&ctx_fut, stream).await {
                    notify_thread_crash(format!("IPC client"), Box::new(e), None).await;
                }
            }
            unreachable!()
        }

        /// Sends an IPC command to the bot.
        pub fn send<T: ::std::fmt::Display, I: IntoIterator<Item = T>>(cmd: I) -> Result<String, Error> { //TODO rename to send_sync and add async variant?
            let mut stream = ::std::net::TcpStream::connect(addr())?;
            writeln!(&mut stream, "{}", cmd.into_iter().map(|arg| ::serenity_utils::shlex::quote(&arg.to_string()).into_owned()).collect::<Vec<_>>().join(" "))?;
            let mut buf = String::default();
            ::std::io::BufReader::new(stream).read_line(&mut buf)?;
            if buf.pop() != Some('\n') { return Err(Error::MissingNewline) }
            Ok(buf)
        }

        #(
            #commands
        )*

        #[macro_export] macro_rules! ipc_client_lib {
            () => {
                use ::std::io::prelude::*;
                #uses

                #port

                /// An error that can occur in an IPC command.
                #[derive(Debug, ::serenity_utils::derive_more::From)]
                pub enum Error {
                    #[allow(missing_docs)]
                    Io(::std::io::Error),
                    /// The command reply did not end in a line break.
                    MissingNewline,
                    /// The bot replied with something other than the expected reply.
                    WrongReply {
                        /// The expected reply.
                        expected: String,
                        /// The reply that was actually received.
                        received: String,
                    },
                }

                impl ::std::fmt::Display for Error {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        match self {
                            Error::Io(e) => e.fmt(f),
                            Error::MissingNewline => write!(f, "the reply to an IPC command did not end in a newline"),
                            Error::WrongReply { expected, received } => write!(f, "unexpected IPC command reply: expected {:?}, received {:?}", expected, received),
                        }
                    }
                }

                #addr_fn

                fn send(cmd: Vec<String>) -> Result<String, Error> {
                    let mut stream = ::std::net::TcpStream::connect(addr())?;
                    writeln!(&mut stream, "{}", cmd.into_iter().map(|arg| ::serenity_utils::shlex::quote(&arg).into_owned()).collect::<Vec<_>>().join(" "))?;
                    let mut buf = String::default();
                    ::std::io::BufReader::new(stream).read_line(&mut buf)?;
                    if buf.pop() != Some('\n') { return Err(Error::MissingNewline) }
                    Ok(buf)
                }

                #(
                    #client_fns
                )*
            };
        }
    })
}

enum SlashCommandDirective {
    Description(String),
    Range(RangeInclusive<i32>),
}

impl Parse for SlashCommandDirective {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let kind = input.parse::<Ident>()?;
        input.parse::<Token![=]>()?;
        Ok(match &*kind.to_string() {
            "description" => Self::Description(input.parse::<LitStr>()?.value()),
            "range" => {
                let lower = input.parse::<LitInt>()?.base10_parse()?;
                input.parse::<Token![..=]>()?;
                let upper = input.parse::<LitInt>()?.base10_parse()?;
                Self::Range(lower..=upper)
            }
            _ => return Err(syn::Error::new(kind.span(), "unknown slash command directive")),
        })
    }
}

#[proc_macro_attribute]
pub fn slash_command(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut args = parse_macro_input!(args as AttributeArgs);
    if args.is_empty() { return quote!(compile_error!("must provide guild ID as argument");).into() }
    let guild_id = args.remove(0);
    let mut perms = quote!(::serenity_utils::slash::CommandPermissions::default());
    for arg in args {
        if_chain! {
            if let NestedMeta::Meta(Meta::List(MetaList { ref path, ref nested, .. })) = arg;
            if path.is_ident("allow");
            then {
                for perm in nested {
                    perms = quote!((#perms | #perm));
                }
            } else {
                return quote_spanned! {arg.span()=>
                    compile_error!("unexpected #[serenity_utils::slash_command] argument");
                }.into()
            }
        }
    }
    let mut cmd_fn = parse_macro_input!(item as ItemFn);
    let name_snake = &cmd_fn.sig.ident;
    let name_kebab = name_snake.to_string().to_case(Case::Kebab);
    let description = if let Ok(doc_comment) = cmd_fn.attrs.iter().filter(|attr| attr.path.is_ident("doc")).exactly_one() {
        match doc_comment.parse_meta() {
            Ok(Meta::NameValue(MetaNameValue { lit: Lit::Str(comment), .. })) => quote!(setup.description(#comment);),
            Ok(_) => return quote_spanned! {doc_comment.span()=>
                compile_error!("unexpected format for doc comment");
            }.into(),
            Err(e) => return e.into_compile_error().into(),
        }
    } else {
        quote!()
    };
    let mut create_options = Vec::default();
    let mut fn_args = Vec::default();
    for arg in &mut cmd_fn.sig.inputs {
        match arg {
            FnArg::Typed(PatType { attrs, pat, ty, .. }) => {
                let opt_name = if let Pat::Ident(PatIdent { ref ident, .. }) = **pat {
                    Some(ident.to_string())
                } else {
                    None
                };
                let mut range_check = quote!(true);
                let mut create_option = Vec::default();
                //TODO use Vec::drain_filter when stabilized
                let mut i = 0;
                while i < attrs.len() {
                    if attrs[i].path.is_ident("serenity_utils") {
                        match attrs.remove(i).parse_args_with(Punctuated::<SlashCommandDirective, Token![,]>::parse_terminated) {
                            Ok(directives) => for directive in directives {
                                match directive {
                                    SlashCommandDirective::Description(desc) => create_option.push(quote!(opt.description(#desc);)),
                                    SlashCommandDirective::Range(range) => {
                                        let lower = i64::from(*range.start());
                                        let upper = i64::from(*range.end());
                                        range_check = quote!((#lower..=#upper).contains(&n));
                                        for n in range {
                                            let n_str = n.to_string();
                                            create_option.push(quote!(opt.add_int_choice(#n_str, #n);));
                                        }
                                    }
                                }
                            },
                            Err(e) => return e.into_compile_error().into(),
                        }
                    } else {
                        i += 1;
                    }
                }
                let (register_option, fn_arg) = loop { //HACK: use a loop with multiple if chains breaking out of it to avoid multiple or nested else clauses
                    if_chain! {
                        if let Type::Path(TypePath { qself: None, ref path }) = **ty;
                        if path.is_ident("GuildId");
                        then {
                            break (false, quote!(if let Some(guild_id) = interaction.guild_id {
                                guild_id
                            } else {
                                interaction.create_interaction_response(ctx, |resp| resp
                                    .interaction_response_data(|data| data
                                        .content("This command only works in a server.")
                                        .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                                    )
                                ).await?;
                                return ::core::result::Result::Ok(())
                            }))
                        }
                    }
                    if_chain! {
                        if let Type::Path(TypePath { qself: None, ref path }) = **ty;
                        if path.is_ident("i64");
                        then {
                            let opt_name = if let Some(ref opt_name) = opt_name {
                                opt_name
                            } else {
                                return quote_spanned! {pat.span()=>
                                    compile_error!("slash command option must be named");
                                }.into()
                            };
                            create_option.push(quote!(opt.kind(::serenity_utils::slash::ApplicationCommandOptionType::Integer);));
                            create_option.push(quote!(opt.required(true);));
                            break (true, quote!({
                                let option = interaction.data.options.remove(0); //TODO error instead of panicking on missing option
                                if let ::serenity_utils::slash::ApplicationCommandInteractionDataOption { name, resolved: ::core::option::Option::Some(::serenity_utils::slash::ApplicationCommandInteractionDataOptionValue::Integer(n)), .. } = option {
                                    if name == #opt_name {
                                        if #range_check {
                                            n
                                        } else {
                                            return ::core::result::Result::Err(::serenity_utils::slash::ParseError::IntegerRange.into())
                                        }
                                    } else {
                                        return ::core::result::Result::Err(::serenity_utils::slash::ParseError::OptionName.into())
                                    }
                                } else {
                                    return ::core::result::Result::Err(::serenity_utils::slash::ParseError::OptionType.into())
                                }
                            }))
                        }
                    }
                    if_chain! {
                        if let Type::Reference(TypeReference { mutability: None, ref elem, .. }) = **ty;
                        if let Type::Path(TypePath { qself: None, ref path }) = **elem;
                        if path.is_ident("Context");
                        then {
                            break (false, quote!(ctx))
                        }
                    }
                    if_chain! {
                        if let Type::Reference(TypeReference { mutability, ref elem, .. }) = **ty;
                        if let Type::Path(TypePath { qself: None, ref path }) = **elem;
                        if path.is_ident("Member");
                        then {
                            break (false, quote!(if let Some(ref #mutability member) = interaction.member {
                                member
                            } else {
                                interaction.create_interaction_response(ctx, |resp| resp
                                    .interaction_response_data(|data| data
                                        .content("This command only works in a server.")
                                        .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                                    )
                                ).await?;
                                return ::core::result::Result::Ok(())
                            }))
                        }
                    }
                    return quote_spanned! {ty.span()=>
                        compile_error!("unsupported argument type for slash command");
                    }.into()
                };
                fn_args.push(fn_arg);
                if register_option {
                    let opt_name = if let Some(opt_name) = opt_name {
                        opt_name
                    } else {
                        return quote_spanned! {pat.span()=>
                            compile_error!("slash command option must be named");
                        }.into()
                    };
                    create_option.push(quote!(opt.name(#opt_name);));
                    create_options.push(create_option);
                }
            }
            FnArg::Receiver(_) => return quote_spanned! {arg.span()=>
                compile_error!("slash commands can't take `self`");
            }.into(),
        }
    }
    let wrapper_name = Ident::new(&format!("{}_wrapper", name_snake), Span::call_site());
    TokenStream::from(quote! {
        #cmd_fn

        fn #wrapper_name(ctx: &Context, mut interaction: ::serenity_utils::slash::ApplicationCommandInteraction) -> ::core::pin::Pin<::std::boxed::Box<dyn ::core::future::Future<Output = ::core::result::Result<(), ::std::boxed::Box<dyn ::std::error::Error + ::core::marker::Send + ::core::marker::Sync>>> + ::core::marker::Send + '_>> {
            //HACK put use and macro in a scope to avoid “inventory is defined multiple times”

            use ::serenity_utils::inventory; // inventory macros assume the crate is in scope

            inventory::submit! {
                ::serenity_utils::slash::Command {
                    guild_id: #guild_id,
                    name: #name_kebab,
                    perms: || #perms,
                    setup: |setup| {
                        setup.name(#name_kebab);
                        setup.default_permission(false);
                        #description
                        #(
                            setup.create_option(|opt| {
                                #(#create_options)*
                                opt
                            });
                        )*
                        setup
                    },
                    handle: #wrapper_name,
                }
            }

            ::std::boxed::Box::pin(async move {
                let fut = #name_snake(#(#fn_args,)*);
                //TODO make sure no extra options are passed (error if !interaction.data.options.is_empty())
                ::serenity_utils::slash::Responder::respond(fut.await, ctx, &interaction).await
            })
        }
    })
}

#[proc_macro_attribute]
pub fn main(args: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as AttributeArgs);
    let mut ipc_mod = None;
    for arg in args {
        if let NestedMeta::Meta(Meta::NameValue(MetaNameValue { ref path, ref lit, .. })) = arg {
            if let Some(ident) = path.get_ident() {
                match &*ident.to_string() {
                    "ipc" => if let Lit::Str(lit) = lit {
                        match lit.parse::<Path>() {
                            Ok(code) => ipc_mod = Some(code),
                            Err(e) => return e.to_compile_error().into(),
                        }
                    } else {
                        return quote_spanned! {lit.span()=>
                            compile_error!("the path to the IPC module must be quoted as a string literal");
                        }.into()
                    },
                    _ => return quote_spanned! {arg.span()=>
                        compile_error!("unexpected serenity_utils::main attribute argument");
                    }.into(),
                }
                continue
            }
        }
        return quote_spanned! {arg.span()=>
            compile_error!("unexpected serenity_utils::main attribute argument");
        }.into()
    }
    let main_fn = parse_macro_input!(item as ItemFn);
    let inner_ret = &main_fn.sig.output;
    let inner_body = main_fn.block;
    let (wrapper_ret, builder_expr) = match main_fn.sig.output {
        ReturnType::Default => return quote_spanned! {main_fn.sig.span()=>
            compile_error!("#[serenity_utils::main] must return a serenity_utils::Builder");
        }.into(),
        ReturnType::Type(rarrow, ref ty) => match **ty {
            Type::Path(ref type_path @ TypePath { qself: None, path: Path { ref segments, .. } })
            if segments.len() == 1 && segments[0].ident == "Result" => {
                let mut type_path = type_path.clone();
                match type_path.path.segments[0].arguments {
                    PathArguments::AngleBracketed(ref mut args) => args.args[0] = parse_quote!(()),
                    _ => return quote_spanned! {main_fn.sig.span()=>
                        compile_error!("missing type parameters for Result in #[serenity_utils::main] return type");
                    }.into(),
                }
                (ReturnType::Type(rarrow, Box::new(Type::Path(type_path))), quote!(main_inner().await?))
            }
            _ => (parse_quote!(-> ::serenity_utils::serenity::Result<()>), quote!(main_inner().await)),
        },
    };
    let wrapper_body = if let Some(ipc_mod) = ipc_mod {
        quote!({
            let mut args = ::std::env::args()
                .skip(1) // ignore executable name
                .peekable();
            if args.peek().is_some() {
                println!("{}", #ipc_mod::send(args)?);
                Ok(())
            } else {
                let mut builder = #builder_expr;
                if builder.has_ctx_fut() {
                    // listen for IPC commands
                    builder = builder.task(|ctx_fut, notify_thread_crash| async move {
                        match #ipc_mod::listen(ctx_fut, &notify_thread_crash).await {
                            Ok(never) => match never {},
                            Err(e) => {
                                eprintln!("{}", e);
                                notify_thread_crash(format!("IPC"), Box::new(e), None).await;
                            }
                        }
                    });
                }
                for cmd in inventory::iter::<::serenity_utils::slash::Command> {
                    builder = ::serenity_utils::handler::HandlerMethods::slash_command(builder, cmd.clone());
                }
                builder.run().await?;
                ::core::result::Result::Ok(())
            }
        })
    } else {
        quote!({
            let builder = #builder_expr;
            builder.run().await?;
            ::core::result::Result::Ok(())
        })
    };
    TokenStream::from(quote! {
        use ::serenity_utils::inventory; // inventory macros assume the crate is in scope

        async fn main_inner() #inner_ret #inner_body

        fn main() #wrapper_ret {
            ::serenity_utils::tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build().expect("failed to set up tokio runtime in serenity_utils::main")
                .block_on(async {
                    #wrapper_body
                })
        }
    })
}
