#![deny(rust_2018_idioms, unused, unused_import_braces, unused_qualifications, warnings)]

//extern crate proc_macro;

use {
    proc_macro::TokenStream,
    quote::quote,
    syn::{
        FnArg,
        ItemConst,
        ItemFn,
        ItemUse,
        parse::{
            ParseStream,
            Parser as _,
        },
    },
};

fn parser(input: ParseStream<'_>) -> Result<(ItemUse, ItemConst, Vec<ItemFn>), syn::Error> {
    let uses = input.parse::<ItemUse>()?;
    let port_const = input.parse::<ItemConst>()?;
    let mut commands = vec![];
    while !input.is_empty() {
        commands.push(input.parse()?);
    }
    Ok((uses, port_const, commands))
}

#[proc_macro]
pub fn ipc(input: TokenStream) -> TokenStream {
    let (uses, port, commands) = match parser.parse(input) {
        Ok(commands) => commands,
        Err(e) => return e.to_compile_error().into()
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
                FnArg::Typed(arg) => arg
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
                    FnArg::Typed(arg) => arg
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
                            expected: format!(#cmd_name)
                        });
                    }
                    Ok(())
                }
            }
        })
        .collect::<Vec<_>>();
    TokenStream::from(quote! {
        use {
            ::std::io::prelude::*,
            ::serenity_utils::futures::prelude::*,
            ::serenity_utils::tokio::prelude::*,
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
            Shlex(::serenity_utils::shlex::Error, String),
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
                    Error::Shlex(e, line) => write!(f, "failed to parse IPC command line: {}: {}", e, line),
                    Error::UnknownCommand(args) => write!(f, "unknown command: {:?}", args),
                }
            }
        }

        /// The address and port where the bot listens for IPC commands.
        fn addr() -> ::std::net::SocketAddr {
            ::std::net::SocketAddr::from(([127, 0, 0, 1], PORT))
        }

        async fn handle_client(ctx_fut: &::serenity_utils::RwFuture<::serenity::client::Context>, stream: ::serenity_utils::tokio::net::TcpStream) -> Result<(), Error> {
            let mut last_error = Ok(());
            let mut buf = String::default();
            let (reader, mut writer) = stream.into_split();
            let mut lines = ::serenity_utils::tokio::io::AsyncBufReadExt::lines(::serenity_utils::tokio::io::BufReader::new(reader));
            while let Some(line) = lines.next().await {
                let line = match line {
                    Ok(line) => line,
                    Err(e) => if e.kind() == ::std::io::ErrorKind::ConnectionReset {
                        break; // connection reset by peer, consider the IPC session terminated
                    } else {
                        return Err(Error::Io(e));
                    }
                };
                buf.push_str(&line);
                let args = match ::serenity_utils::shlex::split(&buf) {
                    Ok(args) => {
                        last_error = Ok(());
                        buf.clear();
                        args
                    }
                    Err(e) => {
                        last_error = Err(Error::Shlex(e, line));
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

        pub async fn listen<Fut: ::std::future::Future<Output = ()>>(ctx_fut: ::serenity_utils::RwFuture<::serenity::client::Context>, notify_thread_crash: &impl Fn(::serenity_utils::RwFuture<::serenity::client::Context>, String, Error) -> Fut) -> Result<(), ::std::io::Error> { //TODO change return type to Result<!, ::std::io::Error>
            let mut listener = ::serenity_utils::tokio::net::TcpListener::bind(addr()).await?;
            while let Some(stream) = listener.next().await {
                let stream = match stream.map_err(Error::Io) {
                    Ok(stream) => stream,
                    Err(e) => {
                        notify_thread_crash(ctx_fut.clone(), format!("IPC client"), e).await;
                        continue
                    }
                };
                if let Err(e) = handle_client(&ctx_fut, stream).await {
                    notify_thread_crash(ctx_fut.clone(), format!("IPC client"), e).await;
                }
            }
            unreachable!();
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
                        received: String
                    }
                }

                impl ::std::fmt::Display for Error {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        match self {
                            Error::Io(e) => e.fmt(f),
                            Error::MissingNewline => write!(f, "the reply to an IPC command did not end in a newline"),
                            Error::WrongReply { expected, received } => write!(f, "unexpected IPC command reply: expected {:?}, received {:?}", expected, received)
                        }
                    }
                }

                /// The address and port where the bot listens for IPC commands.
                fn addr() -> ::std::net::SocketAddr {
                    ::std::net::SocketAddr::from(([127, 0, 0, 1], PORT))
                }

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
