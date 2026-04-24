uniform sampler2D s0;

static const float4 c10 = float4(0.0005, 1.0, 0.5, -2.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t1 : TEXCOORD1;
    float4 t2 : TEXCOORD2;
    float4 t3 : TEXCOORD3;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = (1.0 / input.t3.w);
    r0.x = (input.t3.x * r0.w + c10.y);
    r0.y = (input.t3.y * r0.w + c10.y);
    r0.x = (r0.x * c6.z);
    r0.z = c10.z;
    r0.z = (r0.x * r0.z + c6.x);
    r0.x = (r0.y * -(c10.z) + c10.y);
    r0.xy = (r0.zx * c6.ww + c6.yy);
    r0 = tex2D(s0, r0.xy);
    r1.xyz = (-(input.t2.xyz) + c3.xyz);
    r1.x = dot(r1.xyz, r1.xyz);
    r1.x = rsqrt(r1.x);
    r1.x = (1.0 / r1.x);
    r1.x = (r1.x + -(c7.y));
    r1.x = saturate((r1.x * c1.x));
    r2.xyz = (-(input.t2.xyz) + c4.xyz);
    r2.w = dot(r2.xyz, r2.xyz);
    r2.w = rsqrt(r2.w);
    r2.xyz = (r2.xyz * r2.www);
    r2.w = (1.0 / r2.w);
    r3.xyz = normalize(input.t1.xyz);
    r1.y = dot(r3.xyz, r2.xyz);
    r1.z = (r2.w * -(c10.x) + c10.y);
    r1.y = saturate((r1.y * r1.z));
    r2 = (r1.yyyy * input.v0);
    r2 = (r2 * c5);
    r2 = (c0.xxxx >= 0 ? r2 : input.v0);
    r3 = lerp(r2, r0, r1.xxxx);
    r0 = (c2.xxxx >= 0 ? r3 : r2);
    r1.xyz = (r0.xyz * c10.www + c10.yyy);
    r1.xyz = (c9.yyy * r1.xyz + r0.xyz);
    r1.w = dot(c8, r0);
    r0.xyz = lerp(r1.xyz, r1.www, c9.xxx);
    r0.xyz = (r0.xyz + c9.zzz);
    r0.xyz = (r0.xyz + -(c9.www));
    r1.xyz = lerp(c10.yyy, r0.xyz, input.v0.www);
    r1.w = input.v0.w;
    output.oC0 = r1;
    return output;
}
