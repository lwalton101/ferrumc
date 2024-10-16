use crate::encode::{NetEncode, NetEncodeOpts, NetEncodeResult};
use crate::net_types::var_int::VarInt;
use std::io::Write;
use tokio::io::{AsyncWrite, AsyncWriteExt};

macro_rules! impl_for_primitives {
    ($($primitive_type:ty | $alt:ty),*) => {
        $(
            impl NetEncode for $primitive_type {
                fn encode<W: Write>(&self, writer: &mut W, _: &NetEncodeOpts) -> NetEncodeResult<()> {
                    writer.write_all(&self.to_be_bytes())?;
                    Ok(())
                }

                async fn encode_async<W: tokio::io::AsyncWrite + Unpin>(&self, writer: &mut W, _: &NetEncodeOpts) -> NetEncodeResult<()> {
                    writer.write_all(&self.to_be_bytes()).await?;
                    Ok(())
                }
            }
        
            impl NetEncode for $alt {
                fn encode<W: Write>(&self, writer: &mut W, opts: &NetEncodeOpts) -> NetEncodeResult<()> {
                    // Basically use the encode method of the primitive type,
                    // by converting alt -> primitive and then encoding.
                    (*self as $primitive_type).encode(writer, opts)
                }

                async fn encode_async<W: tokio::io::AsyncWrite + Unpin>(&self, writer: &mut W, opts: &NetEncodeOpts) -> NetEncodeResult<()> {
                    (*self as $primitive_type).encode_async(writer, opts).await
                }
            }
        
        )*
    };
}

impl_for_primitives!(
    u8 | i8,
    u16 | i16,
    u32 | i32,
    u64 | i64,
    u128 | i128,
    f32 | f64
);

impl NetEncode for bool {
    fn encode<W: Write>(&self, writer: &mut W, _: &NetEncodeOpts) -> NetEncodeResult<()> {
        (*self as u8).encode(writer, &NetEncodeOpts::None)
    }

    async fn encode_async<W: AsyncWrite + Unpin>(&self, writer: &mut W, opts: &NetEncodeOpts) -> NetEncodeResult<()> {
        (*self as u8).encode_async(writer, opts).await
    }
}

impl NetEncode for String {
    fn encode<W: Write>(&self, writer: &mut W, _: &NetEncodeOpts) -> NetEncodeResult<()> {
        self.as_str().encode(writer, &NetEncodeOpts::None)
    }
    
    async fn encode_async<W: AsyncWrite + Unpin>(&self, writer: &mut W, _: &NetEncodeOpts) -> NetEncodeResult<()> {
        self.as_str().encode_async(writer, &NetEncodeOpts::None).await
    }
}

impl<'a> NetEncode for &'a str {
    fn encode<W: Write>(&self, writer: &mut W, _: &NetEncodeOpts) -> NetEncodeResult<()> {
        let len: VarInt = VarInt::new(self.len() as i32);
        len.encode(writer, &NetEncodeOpts::None)?;
        writer.write_all(self.as_bytes())?;
        Ok(())
    }

    async fn encode_async<W: AsyncWrite + Unpin>(&self, writer: &mut W, _: &NetEncodeOpts) -> NetEncodeResult<()> {
        let len: VarInt = VarInt::new(self.len() as i32);
        len.encode_async(writer, &NetEncodeOpts::None).await?;
        writer.write_all(self.as_bytes()).await?;
        Ok(())
    }
}