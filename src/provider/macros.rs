macro_rules! provider_dispatcher {
            ($name: ident, $self:ident,  $($provider:ident),* ;$($method:tt)*) => {

                //nested macro
                macro_rules! method_name {
                    ($x:ident) => {
                        return $x.$($method)*
                    };
                }
                //provider
                match $self {
                    $(
                        $name::$provider(x) => {method_name!(x);},
                    )*
                }
            };
        }

macro_rules! dispatch_if_match_static {
    ($name: ident, $url: expr, $provider: ident) => {
        let provider = $provider {};
        for regex in provider.domain_whitelist_regexes() {
            if regex.is_match(&$url.to_string()) {
                debug!("using {}", stringify!($provider));
                return $name::$provider(provider);
            }
        }
    };
}

macro_rules! generate_static_dispatcher {
    ($name:ident for $($provider:ident),* $(,)?) => {

#[async_trait]
impl MediaProvider for $name {
    async fn generate_rss_feed(&self, channel_url: Url) -> eyre::Result<String> {
        provider_dispatcher!($name, self $(,$provider)* ; generate_rss_feed(channel_url).await);
    }

    async fn get_stream_url(&self, media_url: &Url) -> eyre::Result<Url> {
        provider_dispatcher!($name, self $(,$provider)* ; get_stream_url(&media_url).await);
    }

    fn domain_whitelist_regexes(&self) -> Vec<Regex> {
        provider_dispatcher!($name, self $(,$provider)* ; domain_whitelist_regexes());
    }
}

pub fn from(url: &Url) -> $name {
    $(
    dispatch_if_match_static!($name, url, $provider);
    )*
    debug!("using GenericProvider as provider");
    return $name::GenericProvider(GenericProvider);
}

pub enum $name {
    $(
    $provider($provider),
    )*
}
    };
}

