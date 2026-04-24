uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c5 = float4(1.0, 0.5, 0.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
    float4 t1 : TEXCOORD1;
    float4 t2 : TEXCOORD2;
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

    r0.xyz = (-(input.t1.xyz) + c2.xyz);
    r0.x = dot(r0.xyz, r0.xyz);
    r0.x = rsqrt(r0.x);
    r0.x = (1.0 / r0.x);
    r0.x = (r0.x + -(c4.y));
    r0.x = saturate((r0.x * c0.x));
    r0.y = (1.0 / input.t2.w);
    r0.z = (input.t2.x * r0.y + c5.x);
    r0.y = (input.t2.y * r0.y + c5.x);
    r0.z = (r0.z * c3.z);
    r0.w = c3.x;
    r0.w = (r0.z * c5.y + r0.w);
    r0.z = (r0.y * -(c5.y) + c5.x);
    r1.xy = (r0.wz * c3.ww + c3.yy);
    r1 = tex2D(s1, r1.xy);
    r2 = tex2D(s0, input.t0.xy);
    r0.yzw = (input.v0.wzy * -(r2.wzy) + r1.wzy);
    r1 = (r2 * input.v0);
    r0.xyz = (r0.xxx * r0.wzy + r1.xyz);
    r0.xyz = (c1.xxx >= 0 ? r0.xyz : r1.xyz);
    r1.xyz = (r1.www * r0.xyz);
    output.oC0 = r1;
    return output;
}
